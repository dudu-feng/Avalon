import json
from tool import base_tool
from llm import llm

def chat_result_transform(chat_result_content: dict, chat_result ) -> dict:
    token_usage = {}
    if hasattr(chat_result, 'usage_metadata') and chat_result.usage_metadata:
        token_usage = chat_result.usage_metadata
    
    return {
        "role": "assistant",
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
            chat_result_content = json.loads(chat_result.content)
            print(f"\nchat_result_content: {chat_result_content}")
            print(f"\nAgent: {chat_result_content['message']}")
            chat_history.append(chat_result_transform(chat_result_content, chat_result))
        except json.JSONDecodeError:
            print(f"\nAgent: \n chat模型返回content的json解析异常，返回如下： {chat_result.content}")
            chat_history.append({f"role": "assistant", "content": f"chat模型返回content的json解析异常，返回如下： \n {chat_result.content}"})
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
                    action_result_content = json.loads(action_result.content)
                    print(f"\naction_result_content: {action_result_content}")
                    print(f"\nAgent[action分析]: {action_result_content['analysis']}")
                    action_history.append(action_result_transform(action_result_content, action_result))
                except json.JSONDecodeError:
                    return {f"role": "assistant", "content": f"(action步骤analysis的json解析异常){action_result.content}"}
                
                if action_result_content.get("next") == "finished":
                    action_history.append({
                        "action_type": "finished",
                        "action_analysis": action_result_content.get("analysis", "")
                    })
                    break
                elif action_result_content.get("next") == "tool_call":
                    tool_call = action_result_content.get("tool_call", {})
                    tool_name = tool_call.get("name")
                    arguments = tool_call.get("arguments", {})
                    print(f"\nAgent: 调用工具：{tool_name} 参数： {arguments}")
                    
                    tool_result = execute_tool(tool_name, arguments)
                    print(f"\nAgent: 工具调用结果：{tool_result}")

                    action_history.append({
                        "action_type": "tool_call",
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
                        "action_analysis": action_result_content.get("analysis", ""),
                        "sub_analysis": sub_analysis
                    })
                    continue             
                else:
                    action_history.append({
                        "action_type": "error",
                        "action_analysis": action_result_content.get("analysis", "")
                    })
                    break

            chat_history.append({ "role": "assistant", "content": f"【执行记录】\n{action_history}" })
            
    return chat_history


def execute_tool(tool_name: str, arguments: dict) -> str:
    for tool in base_tool.TOOLS:
        if tool.name == tool_name:
            try:
                return tool.invoke(arguments)
            except Exception as e:
                return f"工具调用失败: {e}"
    
    return f"未找到工具: {tool_name}"
  