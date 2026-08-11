import json
import re
import datetime
from tool import base_tool
from llm import llm


def _extract_json_block(text: str) -> str:
    """
    从任意文本中提取最外层的 JSON 对象。

    策略：找到第一个 `{` 和最后一个 `}`，用括号计数确保配对。
    比正则更可靠，能处理嵌套结构和字符串中的花括号。
    """
    # 先尝试找 markdown 代码块中的 JSON
    m = re.search(r'```(?:json)?\s*\n?(.*?)\n?```', text, re.DOTALL)
    if m:
        text = m.group(1)

    start = text.find("{")
    if start == -1:
        return ""

    # 括号计数，处理字符串中的花括号
    depth = 0
    in_string = False
    escape = False
    end = -1

    for i, ch in enumerate(text):
        if start != -1 and i < start:
            continue
        if escape:
            escape = False
            continue
        if ch == "\\":
            escape = True
            continue
        if ch == '"' and not in_string:
            in_string = True
            continue
        if ch == '"' and in_string:
            in_string = False
            continue
        if in_string:
            continue
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                end = i
                break

    if end == -1:
        return ""

    return text[start:end + 1]


def _repair_json(text: str) -> str:
    """
    修复 LLM 常见的 JSON 格式错误。

    按优先级依次尝试：
    1. 移除尾部逗号（最常见）
    2. 单引号转双引号
    3. 补全截断的 JSON（缺少的 } 和 " ）
    """
    # ① 移除对象/数组中的尾部逗号: ,}  ,]
    text = re.sub(r",\s*([}\]])", r"\1", text)

    # ② 移除行尾尾部逗号（LLM 常在多行 JSON 最后一项后加逗号）
    text = re.sub(r",\s*\n\s*([}\]])", r"\n\1", text)

    # ③ 尝试修复截断：补全缺失的闭合花括号
    open_braces = text.count("{") - text.count("}")
    if open_braces > 0:
        # 检查是否在字符串中间被截断
        in_str = False
        for ch in text:
            if ch == '"':
                in_str = not in_str
        if in_str:
            text += '"'
        text += "}" * open_braces

    return text


def parse_llm_json(llm_output_content: str) -> dict:
    """
    解析 LLM 返回的 JSON 内容，带多重容错修复。

    修复策略（按优先级）：
    1. 去除 markdown 代码块
    2. 括号匹配提取 JSON 对象
    3. 直接 json.loads
    4. 修复尾部逗号后重试
    5. 补全截断的 JSON 后重试
    """
    if not llm_output_content or not isinstance(llm_output_content, str):
        return {}

    content = llm_output_content.strip()
    if not content:
        return {}

    # ① 提取 JSON 块（处理嵌套、markdown）
    extracted = _extract_json_block(content)
    if not extracted:
        # 可能 LLM 没返回花括号，尝试整体解析
        extracted = content

    # ② 直接解析
    try:
        result = json.loads(extracted)
        if isinstance(result, dict):
            return result
        return {}
    except json.JSONDecodeError:
        pass

    # ③ 修复常见错误后重试
    repaired = _repair_json(extracted)
    try:
        result = json.loads(repaired)
        if isinstance(result, dict):
            return result
        return {}
    except json.JSONDecodeError:
        pass

    # ④ 最后手段：尝试找到所有有效的键值对
    # 匹配 "key": value 模式
    partial = {}
    for m in re.finditer(r'"(\w+)"\s*:\s*("(?:[^"\\]|\\.)*"|[^,}\]]+)', repaired):
        key = m.group(1)
        val = m.group(2).strip()
        if val.startswith('"') and val.endswith('"'):
            val = val[1:-1]
        partial[key] = val

    if partial:
        return partial

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
                
                action_result_content = parse_llm_json(action_result.content)
                if not action_result_content:
                    chat_history.append({"role": "assistant", "content": f"(action步骤JSON解析异常){action_result.content}"})
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
  