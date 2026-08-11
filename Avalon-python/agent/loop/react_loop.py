import json
import logging
import re
import datetime
from tool import base_tool
from llm import llm

logger = logging.getLogger(__name__)


def _strip_markdown_fences(text: str) -> str:
    """
    剥离外层 markdown 代码块标记。

    ```json\\n...\\n``` → 提取内部内容
    ```json\\n...       → 移除开头标记（无结尾的情况）
    无代码块              → 原样返回
    """
    # 完整代码块
    m = re.search(r'```(?:json)?\s*\n?(.*?)\n?```', text, re.DOTALL)
    if m:
        return m.group(1).strip()

    # 只有开头标记
    text = re.sub(r'^```(?:json)?\s*\n?', '', text, count=1)

    # 只有结尾标记
    if text.rstrip().endswith('```'):
        text = text.rstrip()[:-3].rstrip()

    return text


def parse_llm_json(llm_output_content: str) -> dict:
    """
    解析 LLM 返回的 JSON 内容。

    action 模型已通过 response_format: json_object 约束输出，
    只需处理一种异常：外层 markdown 代码块包裹。

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

def get_current_time() -> str:
    return datetime.datetime.now().strftime("%Y-%m-%d-%H:%M:%S")

def chat_result_transform(chat_result_content: dict, chat_result ) -> dict:
    token_usage = {}
    if hasattr(chat_result, 'usage_metadata') and chat_result.usage_metadata:
        token_usage = chat_result.usage_metadata
    
    return {
        "role": "assistant",
        "time": get_current_time(),
        "content": chat_result_content.get("message", ""),
        "thought": chat_result_content.get("thought", ""),
        "token_usage": token_usage
    }

def action_result_transform(action_result_content: dict, action_result) -> dict:
    token_usage = {}
    if hasattr(action_result, 'usage_metadata') and action_result.usage_metadata:
        token_usage = action_result.usage_metadata

    return {
        "step": action_result_content.get("next", ""),
        "time": get_current_time(),
        "analysis": action_result_content.get("analysis", ""),
        "action": action_result_content.get( action_result_content.get("next", "unknown") , ""),
        "token_usage": token_usage
    }

def react_loop(user_input: str) -> dict:
    chat_history = []
    chat_history.append({"role": "user", "time": get_current_time(), "content": user_input})
    while True:
        chat_result = llm.llm_chat( user_input, chat_history)
        
        chat_result_content = parse_llm_json(chat_result.content)
        if not chat_result_content:
            # 无法解析为 JSON，当作纯文本回复，直接输出并停止循环
            print(f"\nAvalon >: {chat_result.content}")
            chat_history.append({"role": "assistant", "content": chat_result.content})
            return chat_history

        print(f"\nAvalon >: {chat_result_content.get('message', '')}")
        chat_history.append(chat_result_transform(chat_result_content, chat_result))
    
        if chat_result_content.get("next") == "stop":
            break
        
        if chat_result_content.get("next") == "action":
            action_history = []
            action_target = chat_result_content.get("action_target", "") 
            print(f"\nAgent: 开始执行目标：{action_target}")
            action_history.append({ "action_target": action_target })

            while True:
                action_result = llm.llm_action(user_input, action_target, action_history)
                raw_content = action_result.content or ""
                action_result_content = parse_llm_json(raw_content)

                if not action_result_content:
                    logger.warning(
                        "Action JSON 解析失败 | raw_content 前200字符: %s",
                        raw_content[:200],
                    )
                    chat_history.append({
                        "role": "assistant",
                        "content": f"(action步骤JSON解析异常){raw_content[:200]}",
                    })
                    return chat_history

                print(f"\nAgent[action分析]: {action_result_content.get('analysis', '')}")
                action_history.append(action_result_transform(action_result_content, action_result))
                
                if action_result_content.get("next") == "finished":
                    action_history.append({
                        "time": get_current_time(),
                        "action_type": "finished",
                        "action_analysis": action_result_content.get("analysis", "")
                    })
                    break
                elif action_result_content.get("next") == "tool_call":
                    tool_call = action_result_content.get("tool_call") or {}
                    tool_name = tool_call.get("name")
                    arguments = tool_call.get("arguments", {})
                    print(f"\nAvalon >: 调用工具：{tool_name} 参数： {arguments}")
                    
                    tool_result = execute_tool(tool_name, arguments)
                    print(f"\nAvalon >: 工具调用结果：{tool_result}")

                    action_history.append({
                        "action_type": "tool_call",
                        "time": get_current_time(),
                        "action_analysis": action_result_content.get("analysis", ""),
                        "tool_call": tool_call,
                        "tool_result": tool_result
                    })
                    continue          
                elif action_result_content.get("next") == "sub_analysis":
                    sub_analysis = action_result_content.get("sub_analysis", "")
                    print(f"\nAvalon >: 进一步分析：{sub_analysis}")
                    action_history.append({
                        "action_type": "sub_analysis",
                        "time": get_current_time(),
                        "action_analysis": action_result_content.get("analysis", ""),
                        "sub_analysis": sub_analysis
                    })
                    continue             
                else:
                    action_history.append({
                        "action_type": "error",
                        "time": get_current_time(),
                        "action_analysis": action_result_content.get("analysis", "")
                    })
                    break

            chat_history.append({ "role": "assistant", "content": "【执行记录】", "action_history": action_history })
            print(f"\nAvalon >: 执行记录：{action_history}")
    return chat_history

def execute_tool(tool_name: str, arguments: dict) -> str:
    for tool in base_tool.TOOLS:
        if tool.name == tool_name:
            try:
                return tool.invoke(arguments)
            except Exception as e:
                return f"工具调用失败: {e}"
    
    return f"未找到工具: {tool_name}"
  