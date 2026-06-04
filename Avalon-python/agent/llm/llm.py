import os
from langchain_openai import ChatOpenAI
from langchain_core.prompts import ChatPromptTemplate
from langchain_core.output_parsers import StrOutputParser
from langchain_core.messages import BaseMessage, HumanMessage, SystemMessage, AIMessage

from loop import prompt_assemble, session_manage
from tool import base_tool

try:
    from dotenv import load_dotenv
    load_dotenv()
except ImportError:
    pass

model_type = "default"

response_template = """
    请按照以下格式进行思考和回应：

    - 分析当前情况，思考下一步应该做什么
    - 如果需要执行工具，根据现有工具列表规划工具调用
    - 返回纯JSON格式（不要用markdown代码块包裹），如有markdown格式请放在message字段中：
    {
        "thought": "分析当前情况，思考下一步应该做什么",
        "message": "输出给用户看的消息",
        "next": "action 或 stop",
        "action_target": "当next为action时，需要执行的目标描述(将传给独立的action模型执行分步式操作)"
    }
"""


# 获取环境变量
def get_model() -> ChatOpenAI:
    return ChatOpenAI(
        api_key=os.getenv(f"{model_type}_api_key"),
        model_name=os.getenv(f"{model_type}_model"),
        base_url=os.getenv(f"{model_type}_model_base_url"),
    )

# 切换模型类型
def change_model_type( type: str ):
    global model_type
    model_type = type

def llm_chat( user_input: str, chat_history: list ):
    model = get_model()
    system_prompt = prompt_assemble.assemble_system_prompt()
    tool_list = base_tool.get_tool_list()
    current_session = session_manage.get_current_session()

    system_prompt.append(response_template)
    system_prompt.append(tool_list)
    system_prompt.append("\n=====历史会话记录(terminal.json)=====\n")
    system_prompt.append(current_session)

    # 构造消息列表
    messages = [SystemMessage(content=str(system_prompt))]
    # 添加当前用户输入
    messages.append(HumanMessage(content=user_input))
    messages.append(AIMessage(content=str(chat_history)))

    # 调用模型
    result = model.invoke(messages)

    return result

def llm_action( user_input: str, action_target: str, action_history: list ):
    model = get_model()
    system_prompt = [f"""
        这是一个action步骤模型调用，用于执行部分步式任务，请完成以下目标：
        { action_target }
        返回纯JSON格式（不要用markdown代码块包裹），如有markdown格式请放在analysis字段：
        {{
            "analysis": "分析当前情况，思考下一步应该做什么",
            "next": "tool_call / sub_analysis / finished",
            "tool_call": {{
                "name": "要调用的工具名称",
                "arguments": {{}}
            }},
            "sub_analysis": "子步骤分析/规划返回（仅next=sub_analysis时需要）"
        }}

        本次action步骤执行历史，当操作失败次数过多时，则停止执行操作，不要陷入死循环，根据操作历史返回失败原因，回到对话模型：
        { action_history }
    """
    ]
    tool_list = base_tool.get_tool_list()
    system_prompt.append(tool_list)

    messages = [SystemMessage(content=str(system_prompt))]
    messages.append(HumanMessage(content=user_input))

    # 调用模型
    result = model.invoke(messages)

    return result
