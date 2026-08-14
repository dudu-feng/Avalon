"""
飞书渠道基础适配层。

职责：把飞书消息事件贯通到 agent 主模块（react_loop + session_manage）。

流程：
  InboundMessage → 提取文本 + 构造 user_entry → 对用户消息标记「处理中」表情
  → 线程池执行 react_loop（on_event 跨线程推事件）
  → 每个过程事件实时发一条新消息
  → 持久化会话 + 自动压缩检查
  → 取消「处理中」表情，标记「完成」表情
"""

import asyncio
import json
from datetime import datetime
from typing import Any, Dict, List

from lark_oapi.channel import InboundMessage

from server.feishu_service.config import FeishuConfig
from server.feishu_service.feishu_sdk import get_sdk
from server.logger import logger


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

def _code_block(text: str, limit: int = 4000) -> str:
    """把文本安全包进 markdown 代码块（替换内部 ``` 并限制长度）"""
    safe = text.replace("```", "'''")
    if len(safe) > limit:
        safe = safe[:limit] + "\n…（已截断）"
    return f"```\n{safe}\n```"


def _safe_json(obj) -> str:
    """对象转 JSON 字符串，失败时退回 str()"""
    try:
        return json.dumps(obj, ensure_ascii=False, indent=2)
    except (TypeError, ValueError):
        return str(obj)


def _render_event(event_type: str, data: Dict[str, Any]) -> str:
    """把 ReAct 事件渲染成飞书 markdown（清晰排版）"""
    if event_type == "chat_message":
        # 回复正文，原样输出
        return data.get("delta", "")

    if event_type == "action_start":
        target = data.get("action_target", "")
        return f"## 🎯 {target}" if target else "## 🎯 开始执行"

    if event_type == "action_step":
        analysis = data.get("analysis", "")
        return f"📋 {analysis}" if analysis else ""

    if event_type == "action_tool_call":
        tool_name = data.get("tool_name", "")
        arguments = data.get("arguments") or {}
        lines = [f"🛠️ 调用工具：**{tool_name}**"]
        if arguments:
            lines.append(_code_block(_safe_json(arguments)))
        return "\n".join(lines)

    if event_type == "action_tool_result":
        tool_name = data.get("tool_name", "")
        success = data.get("success", False)
        result = data.get("result", "")
        status = "✅" if success else "❌"
        if len(result) <= 200:
            return f"{status} **{tool_name}**：{result}"
        return f"{status} **{tool_name}** 结果：\n" + _code_block(result)

    if event_type == "action_finished":
        return "✅ 执行完成"

    if event_type == "error":
        return f"⚠️ {data.get('message', '')}"

    return ""


# 飞书 markdown 单条消息长度保护：工具结果可能很长，超长会导致发送被拒
_MAX_MARKDOWN_LEN = 8000


def _truncate_markdown(text: str, limit: int = _MAX_MARKDOWN_LEN) -> str:
    """截断超长文本，避免飞书拒收过长的 markdown 消息"""
    if len(text) <= limit:
        return text
    return text[:limit] + "\n…（内容过长，已截断）"


# ============================================================
#  全局消息队列（跨消息串行化）
# ============================================================

# 所有飞书消息（群聊 + 私聊）共用同一队列：会话记忆全局共享于
# current/feishu.json，若并发处理会竞争同一份文件（丢失更新），
# 因此必须在渠道层串行化。worker 按 FIFO 依次处理每条消息。
_message_queue: asyncio.Queue | None = None
_worker_task: asyncio.Task | None = None


def start_worker() -> None:
    """启动全局消息队列的常驻 worker（startup 阶段调用一次）。"""
    global _message_queue, _worker_task
    if _worker_task is not None and not _worker_task.done():
        return
    _message_queue = asyncio.Queue()
    _worker_task = asyncio.create_task(_worker_loop())


async def stop_worker() -> None:
    """停止 worker（shutdown 阶段调用，取消常驻任务）。"""
    global _worker_task, _message_queue
    if _worker_task is not None and not _worker_task.done():
        _worker_task.cancel()
        try:
            await _worker_task
        except asyncio.CancelledError:
            pass
    _worker_task = None
    _message_queue = None


async def _worker_loop() -> None:
    """常驻 worker：FIFO 依次处理每条飞书消息。"""
    while True:
        msg = await _message_queue.get()
        try:
            await _process_message(msg)
        except Exception:
            logger.exception("处理飞书消息失败")
        finally:
            _message_queue.task_done()


# ============================================================
#  消息处理入口
# ============================================================

async def handle_feishu_message(msg: InboundMessage) -> None:
    """
    飞书消息入口：入队后立即返回，由全局 worker 串行处理。

    这样同一时刻只有一条消息在跑 ReAct，避免并发竞争共享会话文件；
    新到的消息按 FIFO 排队，依次处理。
    """
    if _message_queue is None:
        # worker 未启动（配置未走 startup 流程），退化到同步处理
        logger.warning("消息队列 worker 未启动，退化到同步处理")
        await _process_message(msg)
        return
    _message_queue.put_nowait(msg)


async def _process_message(msg: InboundMessage) -> None:
    """
    处理一条飞书消息：提取 → ReAct 逐事件发新消息 → 持久化。

    流程:
      ① 对用户消息标记「处理中」表情
      ② 线程池执行 react_loop，on_event 跨线程推事件
      ③ 每个过程事件实时 send 一条新消息
      ④ 持久化会话 + 自动压缩检查
      ⑤ 取消「处理中」表情，标记「完成」表情
    """
    sdk = get_sdk()
    if sdk is None:
        return

    user_entry = _build_user_entry(msg)
    text = user_entry["content"].strip()
    if not text:
        return  # 空消息暂不处理

    chat_id = msg.conversation.chat_id
    config = FeishuConfig.from_env()

    # ① 对用户消息标记「处理中」表情（reaction 承担状态标记，无需占位消息）
    processing_reaction_id = None
    if config.processing_reaction:
        try:
            logger.info(f"添加处理中表情{config.processing_reaction}")
            result = await sdk.add_reaction(msg.id, config.processing_reaction)
            if result.success and result.raw:
                processing_reaction_id = result.raw.get("data", {}).get("reaction_id")
        except Exception:
            pass

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

    # ③ 消费事件，每个过程步骤实时发一条新消息
    try:
        while True:
            event_type, data = await queue.get()
            if event_type == "__sentinel__":
                break
            line = _render_event(event_type, data)
            if line:
                result = await sdk.send_message(
                    chat_id, {"markdown": _truncate_markdown(line)}
                )
                if not result.success:
                    break
    except Exception:
        pass

    # ④ 等待 ReAct 完成（确保异常不中断回复流程）
    try:
        await executor_future
    except Exception:
        pass

    # ⑤ 结束：取消「处理中」表情，标记「完成」
    if processing_reaction_id:
        try:
            logger.info(f"取消处理中表情{processing_reaction_id}")
            await sdk.remove_reaction(msg.id, processing_reaction_id)
        except Exception:
            pass
    if config.done_reaction:
        try:
            logger.info(f"添加完成表情{config.done_reaction}")
            await sdk.add_reaction(msg.id, config.done_reaction)
        except Exception:
            pass
