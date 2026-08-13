import datetime
from typing import Callable

from tool import base_tool
from llm import llm
from emitter import ReactEmitter


def get_current_time() -> str:
    return datetime.datetime.now().strftime("%Y-%m-%d-%H:%M:%S")


def default_user_entry(user_input: str) -> dict:
    """默认用户消息封装（无渠道 meta 的终端等渠道使用）"""
    return {"role": "user", "time": get_current_time(), "content": user_input}


def chat_result_transform(chat_result_content: dict, chat_result) -> dict:
    return {
        "role": "assistant",
        "time": get_current_time(),
        "content": chat_result_content.get("message", ""),
        "thought": chat_result_content.get("thought", ""),
        "token_usage": chat_result.usage_metadata
    }

def action_result_transform(action_result_content: dict, action_result) -> dict:
    return {
        "step": action_result_content.get("next", ""),
        "time": get_current_time(),
        "analysis": action_result_content.get("analysis", ""),
        "action": action_result_content.get( action_result_content.get("next", "unknown") , ""),
        "token_usage": action_result.usage_metadata
    }

def react_loop(
    user_input: str,
    user_entry: dict | None = None,
    channel: str = "terminal",
    on_event: Callable[[str, dict], None] | None = None,
) -> dict:
    """ReAct 双层循环（纯事件引擎）。

    只通过 on_event 推送中间态，不做 print / 会话持久化；
    终端 / 飞书等调用方各自消费同一套事件契约。
    """
    emitter = ReactEmitter(on_event)

    chat_history = []
    if user_entry is None:
        user_entry = default_user_entry(user_input)
    chat_history.append(user_entry)

    while True:
        # ===== 对话层 =====
        chat_result = llm.llm_chat(user_input, chat_history, channel)
        chat_result_content = chat_result.content

        if not chat_result_content:
            # 无法解析为 JSON，当作纯文本回复，输出后停止
            emitter.chat_message(chat_result.raw)
            emitter.error(50002, "LLM JSON 解析失败，已当作文本回复")
            chat_history.append({"role": "assistant", "content": chat_result.raw})
            return chat_history

        emitter.chat_message(chat_result_content.get("message", ""))
        chat_history.append(chat_result_transform(chat_result_content, chat_result))

        if chat_result_content.get("next") == "stop":
            break

        if chat_result_content.get("next") == "action":
            # ===== 动作层 =====
            action_history = []
            action_target = chat_result_content.get("action_target", "")
            emitter.action_start(action_target)
            action_history.append({ "action_target": action_target })

            while True:
                action_result = llm.llm_action(user_input, action_target, action_history)
                action_result_content = action_result.content

                if not action_result_content:
                    chat_history.append({
                        "role": "assistant",
                        "content": f"(action步骤JSON解析异常){action_result.raw[:200]}",
                    })
                    emitter.error(50003, "action 步骤 JSON 解析异常")
                    return chat_history

                analysis = action_result_content.get("analysis", "")
                next_step = action_result_content.get("next", "")
                emitter.action_step(analysis, next_step)
                action_history.append(action_result_transform(action_result_content, action_result))

                if next_step == "finished":
                    action_history.append({
                        "time": get_current_time(),
                        "action_type": "finished",
                        "action_analysis": analysis
                    })
                    emitter.action_finished(analysis, action_result.usage_metadata)
                    break
                elif next_step == "tool_call":
                    tool_call = action_result_content.get("tool_call") or {}
                    tool_name = tool_call.get("name")
                    arguments = tool_call.get("arguments", {})
                    emitter.action_tool_call(tool_name, arguments)

                    tool_result = base_tool.invoke_tool(tool_name, arguments)
                    success = not tool_result.startswith(("工具调用失败", "未找到工具"))
                    emitter.action_tool_result(tool_name, success, tool_result)

                    action_history.append({
                        "action_type": "tool_call",
                        "time": get_current_time(),
                        "action_analysis": analysis,
                        "tool_call": tool_call,
                        "tool_result": tool_result
                    })
                    continue
                elif next_step == "sub_analysis":
                    sub_analysis = action_result_content.get("sub_analysis", "")
                    emitter.action_sub_analysis(analysis, sub_analysis)
                    action_history.append({
                        "action_type": "sub_analysis",
                        "time": get_current_time(),
                        "action_analysis": analysis,
                        "sub_analysis": sub_analysis
                    })
                    continue
                else:
                    action_history.append({
                        "action_type": "error",
                        "time": get_current_time(),
                        "action_analysis": analysis
                    })
                    break

            chat_history.append({ "role": "assistant", "content": "【执行记录】", "action_history": action_history })

    return chat_history
