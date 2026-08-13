"""
飞书渠道基础适配层。

职责：把飞书消息事件贯通到 agent 主模块（react_loop + session_manage）。

流程：
  InboundMessage → 提取文本 + 构造 user_entry → 发占位消息
  → 线程池执行 react_loop（on_event 跨线程推事件）
  → 异步消费事件，流式 edit_message 更新回复
  → 持久化会话 + 自动压缩检查
"""

import asyncio
from datetime import datetime
from typing import Any, Dict, List

from lark_oapi.channel import InboundMessage

from server.feishu_service.feishu_sdk import get_sdk


def _now() -> str:
    return datetime.now().strftime("%Y-%m-%d-%H:%M:%S")


# ============================================================
#  消息转换（飞书 → 标准 user entry）
# ============================================================

def _build_content(msg: InboundMessage) -> str:
    """拼接用户消息文本（群聊加发送者前缀）"""
    parts: List[str] = []

    if msg.conversation.chat_type in ("group", "topic"):
        sender_name = msg.sender.display_name or msg.sender.open_id or "未知"
        parts.append(sender_name)

    if msg.content_text:
        if parts:
            parts.append(f": {msg.content_text}")
        else:
            parts.append(msg.content_text)
    else:
        # 纯非文本消息（图片/文件等）
        if parts:
            parts.append(": [非文本消息]")
        else:
            parts.append("[非文本消息]")

    return "".join(parts)


def _build_meta(msg: InboundMessage) -> Dict[str, Any]:
    """构造用户消息的结构化 meta"""
    return {
        "message_id": msg.id,
        "chat_id": msg.conversation.chat_id,
        "chat_type": msg.conversation.chat_type,
        "sender": {
            "open_id": msg.sender.open_id,
            "display_name": msg.sender.display_name,
            "is_bot": msg.sender.is_bot,
        },
        "content_type": msg.raw_content_type,
    }


def _build_user_entry(msg: InboundMessage) -> Dict[str, Any]:
    """InboundMessage → {role, time, content, meta}"""
    return {
        "role": "user",
        "time": _now(),
        "content": _build_content(msg),
        "meta": _build_meta(msg),
    }


# ============================================================
#  事件渲染（ReAct 事件 → 可读文本）
# ============================================================

def _render_event(event_type: str, data: Dict[str, Any]) -> str:
    """把 ReAct 事件渲染成一行可读文本（chat_thought 内部思考不展示）"""
    if event_type == "chat_message":
        return data.get("delta", "")
    if event_type == "action_start":
        return f"🔧 执行目标：{data.get('action_target', '')}"
    if event_type == "action_step":
        analysis = data.get("analysis", "")
        return f"📋 {analysis}" if analysis else ""
    if event_type == "action_tool_call":
        return f"🛠️ 调用工具：{data.get('tool_name', '')}"
    if event_type == "action_tool_result":
        return f"📦 结果：{data.get('result', '')}"
    if event_type == "action_finished":
        return "✅ 执行完成"
    if event_type == "error":
        return f"⚠️ {data.get('message', '')}"
    return ""


# ============================================================
#  消息处理入口
# ============================================================

async def handle_feishu_message(msg: InboundMessage) -> None:
    """
    处理一条飞书消息：提取 → ReAct 流式 → 编辑回复 → 持久化。
    """
    sdk = get_sdk()
    if sdk is None:
        return

    user_entry = _build_user_entry(msg)
    text = user_entry["content"].strip()
    if not text:
        return  # 空消息暂不处理

    chat_id = msg.conversation.chat_id

    # ① 发送占位消息，拿到 message_id 供后续流式编辑
    try:
        send_result = await sdk.send_message(
            chat_id, {"markdown": "思考中…"}, reply_to=msg.id
        )
        if not send_result.success or not send_result.message_id:
            return
        message_id = send_result.message_id
    except Exception:
        return

    # ② 跨线程事件队列
    queue: asyncio.Queue = asyncio.Queue()
    loop = asyncio.get_event_loop()

    def on_event(event_type: str, data: Dict[str, Any]) -> None:
        try:
            loop.call_soon_threadsafe(queue.put_nowait, (event_type, data))
        except RuntimeError:
            pass

    def run_react() -> list:
        """在线程池执行 ReAct + 持久化（同步阻塞操作全部放这里）"""
        from loop import react_loop, session_manage

        try:
            session_manage.init_session("feishu")
            chat_history = react_loop.react_loop(
                text, on_event=on_event, channel="feishu", user_entry=user_entry
            )
            session_manage.update_current_session(chat_history, "feishu")
            session_manage.auto_compress_check_from_history(chat_history, "feishu")
            return chat_history
        finally:
            # 兜底 sentinel：无论成功/异常都通知消费者退出，避免死锁
            on_event("__sentinel__", {})

    executor_future = loop.run_in_executor(None, run_react)

    # ③ 消费事件，流式编辑回复
    lines: List[str] = []
    try:
        while True:
            event_type, data = await queue.get()
            if event_type == "__sentinel__":
                break
            line = _render_event(event_type, data)
            if line:
                lines.append(line)
                await sdk.edit_message(message_id, {"markdown": "\n".join(lines)})
    except Exception:
        pass

    # ④ 等待 ReAct 完成（确保异常不中断回复流程）
    try:
        await executor_future
    except Exception:
        pass

    # ⑤ 最终编辑（确保有内容）
    final = "\n".join(lines) if lines else "(无回复)"
    try:
        await sdk.edit_message(message_id, {"markdown": final})
    except Exception:
        pass
