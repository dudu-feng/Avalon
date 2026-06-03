import json
import re
import datetime
from tool import base_tool
from llm import llm

def _parse_llm_json(raw_content: str) -> dict:
    """
    健壮的 JSON 解析，处理 LLM 返回的各种格式问题：
    1. 去除 markdown 代码块包裹 (```json ... ```)
    2. 去除 "content=" 前缀
    3. 如果顶层有 content 键且值为 dict，解包内层数据
    """
    content = raw_content.strip()

    # 1. 去除 markdown 代码块 ```json ... ``` 或 ``` ... ```
    md_match = re.match(r'^```(?:json)?\s*\n(.*?)\n```\s*$', content, re.DOTALL)
    if md_match:
        content = md_match.group(1).strip()

    # 2. 去除 "content=" 前缀（部分 prompt 模板残留）
    if content.startswith("content="):
        content = content[len("content="):].strip()

    # 3. 如果内容不以 { 开头，尝试定位到第一个 {
    if not content.startswith("{"):
        brace_idx = content.find("{")
        if brace_idx != -1:
            content = content[brace_idx:]

    # 4. 解析 JSON
    parsed = json.loads(content)

    # 5. 如果顶层有 content 键且值是 dict，解包内层
    if isinstance(parsed, dict) and "content" in parsed and isinstance(parsed["content"], dict):
        inner = parsed["content"]
        if any(k in inner for k in ("analysis", "next", "message", "thought")):
            return inner

    return parsed


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

# TODO 给历史添加时间戳
def react_loop(user_input: str) -> dict:
    chat_history = []
    chat_history.append({"role": "user", "content": user_input})
    while True:
        chat_result = llm.llm_chat( user_input, chat_history)
        
        try:
            chat_result_content = _parse_llm_json(chat_result.content)
            print(f"\nchat_result_content: {chat_result_content}")
            print(f"\nAgent: {chat_result_content['message']}")
            chat_history.append(chat_result_transform(chat_result_content, chat_result))
        except (json.JSONDecodeError, ValueError):
            # 无法解析为 JSON，当作纯文本回复，直接输出并停止循环
            print(f"\nAgent: {chat_result.content}")
            chat_history.append({"role": "assistant", "content": chat_result.content})
            return chat_history
    
        if chat_result_content.get("next") == "stop":
            break
        
        if chat_result_content.get("next") == "action":
            action_history = []
            action_target = chat_result_content.get("action_target", "") 
            print(f"\nAgent: 开始执行目标：{action_target}")
            action_history.append({ "action_target": action_target })

            while True:
                action_result = llm.llm_action(user_input, action_target, action_history)
                
                try:
                    action_result_content = _parse_llm_json(action_result.content)
                    print(f"\naction_result_content: {action_result_content}")
                    print(f"\nAgent[action分析]: {action_result_content['analysis']}")
                    action_history.append(action_result_transform(action_result_content, action_result))
                except (json.JSONDecodeError, ValueError):
                    chat_history.append({"role": "assistant", "content": f"(action步骤JSON解析异常){action_result.content}"})
                    return chat_history
                
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
                    print(f"\nAgent: 调用工具：{tool_name} 参数： {arguments}")
                    
                    tool_result = execute_tool(tool_name, arguments)
                    print(f"\nAgent: 工具调用结果：{tool_result}")

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
                    print(f"\nAgent: 进一步分析：{sub_analysis}")
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
            
    return chat_history


def execute_tool(tool_name: str, arguments: dict) -> str:
    for tool in base_tool.TOOLS:
        if tool.name == tool_name:
            try:
                return tool.invoke(arguments)
            except Exception as e:
                return f"工具调用失败: {e}"
    
    return f"未找到工具: {tool_name}"
  