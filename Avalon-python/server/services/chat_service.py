"""
Chat SSE 服务 —— 独立的流式 ReAct 循环 + SSE 桥接

本模块自包含流式 ReAct 逻辑，不修改 agent/loop/react_loop.py。
通过 asyncio.Queue 桥接同步 LLM 调用和异步 FastAPI SSE 响应。
"""

import asyncio
import traceback
import uuid
from datetime import datetime
from typing import AsyncGenerator, Callable, Optional

from llm import llm
from loop.react_loop import (
    action_result_transform,
    chat_result_transform,
    get_current_time,
)
from tool import base_tool

# ============================================================
#  流式 ReAct 循环（独立实现，不修改 agent/loop/react_loop.py）
# ============================================================

def _emit(event_type: str, data: dict, callback: Optional[Callable] = None):
    """向回调推送事件"""
    if callback:
        callback(event_type, data)


def _execute_tool(tool_name: str, arguments: dict) -> str:
    """执行工具（与 react_loop.execute_tool 逻辑一致）。返回值强制转为字符串。"""
    for tool in base_tool.TOOLS:
        if tool.name == tool_name:
            try:
                result = tool.invoke(arguments)
                # 确保返回值为字符串（部分工具如 get_directory_contents 可能返回 list）
                if not isinstance(result, str):
                    import json as _json
                    result = _json.dumps(result, ensure_ascii=False, indent=2)
                return result
            except Exception as e:
                return f"工具调用失败: {e}"
    return f"未找到工具: {tool_name}"


def streaming_react_loop(
    user_input: str,
    on_event: Optional[Callable[[str, dict], None]] = None,
    channel: str = "terminal",
) -> list:
    """
    带事件回调的 ReAct 双层循环。

    与 agent/loop/react_loop.py 的 react_loop() 核心逻辑一致，
    但在每个关键步骤通过 on_event 回调推送进度事件。

    Args:
        user_input: 用户输入
        on_event: 事件回调 (event_type, data_dict)
        channel: 渠道标识 (terminal/lark/desktop/web)

    Returns:
        chat_history — 完整聊天历史列表
    """
    chat_history = []
    chat_history.append({
        "role": "user",
        "time": get_current_time(),
        "content": user_input,
    })

    message_id = str(uuid.uuid4())[:8]
    _emit("chat_start", {"message_id": message_id}, on_event)

    # ===== 对话层循环 =====
    while True:
        chat_result = llm.llm_chat(user_input, chat_history, channel)
        chat_result_content = chat_result.content

        if not chat_result_content:
            # JSON 解析完全失败 → 纯文本回复
            _emit("chat_message", {"delta": chat_result.raw}, on_event)
            _emit("error", {
                "code": 50002,
                "message": "LLM JSON 解析失败，已当作文本回复",
            }, on_event)
            chat_history.append({
                "role": "assistant",
                "content": chat_result.raw,
            })
            _emit("done", {
                "chat_history": chat_history,
                "compress_triggered": False,
            }, on_event)
            return chat_history

        # 部分恢复：如果解析出了 message 但缺少 next，当作 stop 处理
        if not chat_result_content.get("next"):
            chat_result_content["next"] = "stop"

        # thought
        thought = chat_result_content.get("thought", "")
        if thought:
            _emit("chat_thought", {"content": thought}, on_event)

        # message
        message_text = chat_result_content.get("message", "")
        if message_text:
            _emit("chat_message", {"delta": message_text}, on_event)

        chat_history.append(chat_result_transform(
            chat_result_content, chat_result
        ))

        # next=stop → 结束
        if chat_result_content.get("next") == "stop":
            _emit("chat_stop", {"token_usage": chat_result.usage_metadata}, on_event)
            break

        # next=action → 进入动作层
        if chat_result_content.get("next") == "action":
            action_target = chat_result_content.get("action_target", "")
            _emit("action_start", {"action_target": action_target}, on_event)

            action_history = [{"action_target": action_target}]

            # ===== 动作层循环 =====
            while True:
                action_result = llm.llm_action(
                    user_input, action_target, action_history
                )
                action_result_content = action_result.content

                if not action_result_content:
                    # JSON 解析失败 → 记录原始内容，回传给上层对话模型处理
                    _emit("error", {
                        "code": 50002,
                        "message": "action步骤JSON解析异常",
                    }, on_event)
                    chat_history.append({
                        "role": "assistant",
                        "content": f"(action步骤JSON解析异常){action_result.raw[:200]}",
                    })
                    _emit("done", {
                        "chat_history": chat_history,
                        "compress_triggered": False,
                    }, on_event)
                    return chat_history

                # 如果缺少 next 字段，尝试用已有信息推断
                if not action_result_content.get("next"):
                    if action_result_content.get("tool_call", {}).get("name"):
                        action_result_content["next"] = "tool_call"
                    else:
                        action_result_content["next"] = "finished"

                analysis = action_result_content.get("analysis", "")
                next_step = action_result_content.get("next", "")
                _emit("action_step", {
                    "analysis": analysis,
                    "next": next_step,
                }, on_event)

                action_history.append(action_result_transform(
                    action_result_content, action_result
                ))

                if next_step == "finished":
                    action_history.append({
                        "time": get_current_time(),
                        "action_type": "finished",
                        "action_analysis": analysis,
                    })
                    _emit("action_finished", {
                        "analysis": analysis,
                        "token_usage": action_result.usage_metadata,
                    }, on_event)
                    break

                elif next_step == "tool_call":
                    tool_call = action_result_content.get("tool_call") or {}
                    tool_name = tool_call.get("name", "")
                    arguments = tool_call.get("arguments", {})
                    _emit("action_tool_call", {
                        "tool_name": tool_name,
                        "arguments": arguments,
                    }, on_event)

                    tool_result = _execute_tool(tool_name, arguments)
                    _emit("action_tool_result", {
                        "tool_name": tool_name,
                        "success": (
                            not tool_result.startswith("工具调用失败")
                            and not tool_result.startswith("未找到工具")
                        ),
                        "result": tool_result,
                    }, on_event)

                    action_history.append({
                        "action_type": "tool_call",
                        "time": get_current_time(),
                        "action_analysis": analysis,
                        "tool_call": tool_call,
                        "tool_result": tool_result,
                    })
                    continue

                elif next_step == "sub_analysis":
                    sub_analysis = action_result_content.get("sub_analysis", "")
                    _emit("action_sub_analysis", {
                        "analysis": analysis,
                        "sub_analysis": sub_analysis,
                    }, on_event)
                    action_history.append({
                        "action_type": "sub_analysis",
                        "time": get_current_time(),
                        "action_analysis": analysis,
                        "sub_analysis": sub_analysis,
                    })
                    continue

                else:
                    action_history.append({
                        "action_type": "error",
                        "time": get_current_time(),
                        "action_analysis": analysis,
                    })
                    break

            chat_history.append({
                "role": "assistant",
                "content": "【执行记录】",
                "action_history": action_history,
            })

    _emit("done", {
        "chat_history": chat_history,
        "compress_triggered": False,
    }, on_event)
    return chat_history


# ============================================================
#  SSE 桥接层
# ============================================================

async def generate_sse(session_id: str, message: str) -> AsyncGenerator[str, None]:
    """
    SSE 事件流生成器。

    通过 asyncio.Queue 桥接同步的 streaming_react_loop 和异步 SSE:
      - 在线程池中运行 streaming_react_loop
      - on_event 回调跨线程将事件推入 asyncio.Queue
      - 异步读取队列并格式化为 SSE 字符串
    """
    from loop import session_manage
    from server.core.sse import format_sse

    # 从 session_id 提取渠道前缀
    channel = session_id.split("_")[0] if "_" in session_id else "web"

    queue: asyncio.Queue = asyncio.Queue()
    event_loop = asyncio.get_event_loop()

    def on_event(event_type: str, data: dict):
        """同步回调 → 异步队列（线程安全）"""
        try:
            event_loop.call_soon_threadsafe(
                queue.put_nowait, (event_type, data)
            )
        except RuntimeError:
            pass  # 事件循环已关闭

    compress_triggered = False

    def run_react():
        """在线程池中执行"""
        nonlocal compress_triggered
        try:
            history = streaming_react_loop(message, on_event=on_event, channel=channel)

            # 持久化
            session_manage.update_current_session(history, channel)

            # 自动压缩检查
            compress_triggered = (
                session_manage.auto_compress_check_from_history(history, channel)
            )
        except Exception as e:
            traceback.print_exc()
            on_event("error", {
                "code": 50001,
                "message": str(e),
                "detail": traceback.format_exc()[-500:],
            })

    asyncio.get_event_loop().run_in_executor(None, run_react)

    event_id = 0
    while True:
        event_type, data = await queue.get()
        event_id += 1

        # 在 done 事件中补充 compress_triggered 信息
        if event_type == "done":
            data["compress_triggered"] = compress_triggered

        yield format_sse(event_id, event_type, data)

        if event_type in ("done", "error"):
            break
