import json
import re
from langchain_openai import ChatOpenAI
from langchain_core.prompts import ChatPromptTemplate
from langchain_core.output_parsers import StrOutputParser
from langchain_core.messages import BaseMessage, HumanMessage, SystemMessage, AIMessage

from config.env_config import env_config
from loop import prompt_assemble, session_manage
from tool import base_tool
from server.logger import logger

response_template = """
    请按照以下格式进行思考和回应：

    - 分析当前情况，思考下一步应该做什么
    - 如果需要执行工具，根据现有工具列表规划工具调用
    - 返回纯JSON格式：
    样例JSON输出:{
        "thought": "分析当前情况，思考下一步应该做什么",
        "message": "输出给用户看的消息",
        "next": "action 或 stop",
        "action_target": "当next为action时，需要执行的目标描述(将传给独立的action模型执行分步式操作)"
    }
"""

# ========== 模型单例（懒加载，只创建一次） ==========
_model_instance: ChatOpenAI | None = None
_json_model_instance: ChatOpenAI | None = None


class LLMResult:
    """LLM 调用结果封装，内部已完成 JSON 解析"""
    __slots__ = ('content', 'raw', 'usage_metadata')

    def __init__(self, content: dict, raw: str, usage_metadata: dict | None = None):
        self.content = content              # 解析后的 JSON dict（失败时为 {}）
        self.raw = raw                      # 原始响应字符串（解析失败时用于降级输出）
        self.usage_metadata = usage_metadata or {}


def get_model() -> ChatOpenAI:
    """获取会话模型实例（单例，temperature=0.7）"""
    global _model_instance
    if _model_instance is None:
        _model_instance = ChatOpenAI(
            api_key=env_config.default_api_key,
            model_name=env_config.default_model,
            base_url=env_config.default_model_base_url,
            temperature=0.7,
        )
    return _model_instance

def get_jsonOutput_model() -> ChatOpenAI:
    """获取 JSON 输出模型实例（单例，temperature=0.1，用于 action 步骤和会话压缩）"""
    global _json_model_instance
    if _json_model_instance is None:
        _json_model_instance = ChatOpenAI(
            api_key=env_config.default_api_key,
            model_name=env_config.default_model,
            base_url=env_config.default_model_base_url,
            temperature=0.1,
            timeout=120
        )
    return _json_model_instance

def refresh_models():
    """重新创建模型实例（在配置变更后调用）"""
    global _model_instance, _json_model_instance
    _model_instance = None
    _json_model_instance = None


def _strip_markdown_fences(text: str) -> str:
    """剥离外层 markdown 代码块标记"""
    # 完整代码块 ```json\n...\n```
    m = re.search(r'```(?:json)?\s*\n?(.*?)\n?```', text, re.DOTALL)
    if m:
        return m.group(1).strip()

    # 只有开头标记 ```json\n...
    text = re.sub(r'^```(?:json)?\s*\n?', '', text, count=1)

    # 只有结尾标记 ```
    if text.rstrip().endswith('```'):
        text = text.rstrip()[:-3].rstrip()

    return text


def parse_llm_json(llm_output_content: str) -> dict:
    """
    解析 LLM 返回的 JSON 内容。

    步骤：
    1. 直接 json.loads（正常情况）
    2. 剥离 markdown 代码块后 json.loads（被代码块包裹的情况）
    3. 都不行 → 返回 {}，交由上层决定重试或调整
    """
    if not llm_output_content or not isinstance(llm_output_content, str):
        return {}

    content = llm_output_content.strip()
    if not content:
        return {}

    # ① 直接解析
    try:
        result = json.loads(content)
        if isinstance(result, dict):
            return result
    except json.JSONDecodeError:
        pass

    # ② 剥离 markdown 代码块后重试
    stripped = _strip_markdown_fences(content)
    if stripped != content:
        try:
            result = json.loads(stripped)
            if isinstance(result, dict):
                return result
        except json.JSONDecodeError:
            pass

    # ③ 无法解析，返回空 dict 交由上层处理
    return {}


def _invoke_and_parse(model: ChatOpenAI, messages: list) -> LLMResult:
    """调用模型并解析 JSON，统一封装为 LLMResult"""
    result = model.invoke(messages)
    raw = result.content or ""
    usage = getattr(result, 'usage_metadata', None)
    return LLMResult(content=parse_llm_json(raw), raw=raw, usage_metadata=usage)


def llm_chat( user_input: str, chat_history: list, channel: str = "terminal" ) -> LLMResult:
    model = get_model()
    system_prompt = prompt_assemble.assemble_system_prompt()
    tool_list = base_tool.get_tool_list()
    current_session = session_manage.get_session_context_for_prompt(channel)

    system_prompt.append(response_template)
    system_prompt.append(tool_list)
    system_prompt.append(f"\n=====历史会话记录({channel}.json)=====\n")
    system_prompt.append(current_session)

    # 构造消息列表
    messages = [SystemMessage(content=str(system_prompt))]
    # 添加当前用户输入
    messages.append(HumanMessage(content=user_input))
    messages.append(AIMessage(content=str(chat_history)))
    
    logger.info(f"提示词组装完成，开始请求对话层，用户输入：{user_input}")
    # 调用模型并解析
    return _invoke_and_parse(model, messages)

def llm_action( action_target: str, action_history: list ) -> LLMResult:
    model = get_jsonOutput_model()
    tool_list = base_tool.get_tool_list()

    system_prompt = f"""
        这是一个action步骤模型调用，用于执行部分步式任务，请完成以下目标，当操作失败次数过多时，则停止执行操作：
        { action_target }
        遵守规则：
        1. 拒绝发散性思考，只根据执行历史和工具列表进行分析。
        2. 拒绝多次尝试同一错误操作，避免死循环。
        3. 简洁思考，限制思考过程不要太长，保持思考效率。

        返回纯JSON格式（不要用markdown代码块包裹）：
        样例JSON输出:{{
            "analysis": "分析当前情况，思考下一步应该做什么",
            "next": "tool_call / sub_analysis / finished",
            "tool_call": {{
                "name": "要调用的工具名称",
                "arguments": {{}}
            }},
            "sub_analysis": "子步骤分析/规划返回（仅next=sub_analysis时需要）"
        }}

        可用的工具列表：
        { tool_list }

        本次action步骤执行历史，当操作失败次数过多时，则停止执行操作:
        { action_history }
    """

    messages = [SystemMessage(content=system_prompt)]

    logger.info(f"提示词组装完成，开始请求action大模型，用户输入：{action_target}")
    # 调用模型并解析
    return _invoke_and_parse(model, messages)

def llm_compress( session_data: dict ) -> LLMResult:
    model = get_jsonOutput_model()
    system_prompt = f"""
        这是一个压缩模型调用，用于压缩历史会话记录，返回纯JSON格式（不要用markdown代码块包裹）
        {{
            "summary": ["被压缩会话的总结1", "被压缩会话的总结2"],(被压缩会话的总结，因为后续需要把总结字段向量化作后续会话语义检索，所以单个总结长度不超过200个字符，如会话内容较多，可返回多个总结)
            "keywords": ["关键词1", "关键词2", "关键词3", ...](被压缩会话内容的精炼关键词，为了方便后续会话记忆作关键词检索，可以是内容概括性关键词，也可以是重要事件，关键事物的直接指向性关键词)
        }}
    """
    user_prompt = f"""
        压缩以下历史会话：
        { session_data }
    """
    messages = [SystemMessage(content=system_prompt)]
    messages.append(HumanMessage(content=user_prompt))

    # 调用模型并解析
    return _invoke_and_parse(model, messages)
