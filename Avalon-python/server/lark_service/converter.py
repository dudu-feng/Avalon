"""
飞书消息 → 标准 session entry 转换器。

将飞书的 InboundMessage、CardActionEvent、系统事件等
转换为 Avalon 核心引擎可理解的 {role, time, content, meta} 格式。

content 拼接规则详见 doc/design/feishu-session-format.md
"""

import logging
from datetime import datetime
from typing import Any, Dict, Optional

from lark_oapi.channel import (
    InboundMessage,
    CardActionEvent,
    BotAddedEvent,
    BotLeaveEvent,
    ReactionEvent,
    MessageReadEvent,
    CommentEvent,
)


def _now() -> str:
    return datetime.now().strftime("%Y-%m-%d-%H:%M:%S")


def _has_reply_context(msg: InboundMessage) -> bool:
    """是否有回复上下文"""
    return msg.reply is not None and msg.reply.message_id is not None


def _format_size(size_bytes: int) -> str:
    if size_bytes < 1024:
        return f"{size_bytes}B"
    elif size_bytes < 1024 * 1024:
        return f"{size_bytes // 1024}KB"
    else:
        return f"{size_bytes / (1024 * 1024):.1f}MB"


_RESOURCE_TYPE_LABELS = {
    "image": "图片",
    "file": "文件",
    "audio": "语音",
    "video": "视频",
    "sticker": "表情包",
}


def _resource_label(res) -> str:
    """生成资源的人类可读标签"""
    label = _RESOURCE_TYPE_LABELS.get(res.type, "附件")
    if res.file_name:
        label += f": {res.file_name}"
    if hasattr(res, "size_bytes") and res.size_bytes:
        label += f", {_format_size(res.size_bytes)}"
    return label


logger = logging.getLogger(__name__)


class MessageConverter:
    """
    飞书消息 → 标准 session entry 转换器。

    所有方法都是纯函数，无状态，无副作用。
    """

    # ── 主转换入口 ──

    def to_session_entry(self, msg: InboundMessage) -> Dict[str, Any]:
        """
        InboundMessage → {role, time, content, meta}

        返回的 dict 可直接传给 session_manage.update_current_session()
        """
        content = self._build_content(msg)
        meta = self._build_user_meta(msg)

        logger.debug(
            "[Lark] 消息转换完成 | chat_type=%s | content_len=%d | has_reply=%s | resources=%d",
            meta["chat_type"],
            len(content),
            meta.get("reply_to") is not None,
            len(meta.get("resources", [])),
        )
        return {"role": "user", "time": _now(), "content": content, "meta": meta}

    def card_action_to_entry(self, event: CardActionEvent) -> Dict[str, Any]:
        """
        CardActionEvent → {role, time, content, meta}
        """
        operator_name = event.operator.name or "用户"

        # 构建可读的 content
        action_label = event.action.option or event.action.tag or "交互"
        content = f"{operator_name}: [点击了卡片{action_label}]"

        return {
            "role": "user",
            "time": _now(),
            "content": content,
            "meta": {
                "chat_id": event.chat_id,
                "chat_type": "p2p",  # 卡片交互无法确定 chat_type，保守使用 p2p
                "sender": {
                    "open_id": event.operator.open_id,
                    "display_name": operator_name,
                },
                "event_type": "card_action",
                "action": {
                    "tag": event.action.tag,
                    "value": event.action.value,
                    "option": event.action.option,
                },
                "source_message_id": event.message_id,
                "content_type": "card_action",
            },
        }

    def bot_added_to_entry(self, event: BotAddedEvent) -> Dict[str, Any]:
        operator_name = event.operator.name or "管理员"
        chat_name = event.chat_name or "未知群聊"
        return {
            "role": "system",
            "time": _now(),
            "content": f"[系统] 被 {operator_name} 邀请加入群聊 {chat_name}",
            "meta": {
                "chat_id": event.chat_id,
                "event_type": "bot_added",
                "operator": {
                    "open_id": event.operator.open_id,
                    "display_name": operator_name,
                },
                "chat_name": chat_name,
            },
        }

    def bot_leave_to_entry(self, event: BotLeaveEvent) -> Dict[str, Any]:
        operator_name = event.operator.name or "系统"
        return {
            "role": "system",
            "time": _now(),
            "content": f"[系统] 被 {operator_name} 移出群聊",
            "meta": {
                "chat_id": event.chat_id,
                "event_type": "bot_leave",
                "operator": {
                    "open_id": event.operator.open_id,
                    "display_name": operator_name,
                },
            },
        }

    def reaction_to_entry(self, event: ReactionEvent) -> Optional[Dict[str, Any]]:
        return {
            "role": "system",
            "time": _now(),
            "content": f"[系统] 收到 reaction: {event.emoji_type}",
            "meta": {
                "chat_id": event.chat_id,
                "event_type": "reaction",
                "message_id": event.message_id,
                "emoji_type": event.emoji_type,
            },
        }

    # ── content 拼接 ──

    def _build_content(self, msg: InboundMessage) -> str:
        """
        将 InboundMessage 拼接为 LLM 可读的 content 字符串。

        规则：
          群聊 → "[发送者名称]: {正文}"
          单聊 → "{正文}"
          有回复 → 追加引用块
          有资源 → 追加资源标记
        """
        parts = []

        # 1. 发送者前缀（仅群聊和话题）
        if msg.conversation.chat_type in ("group", "topic"):
            sender_name = msg.sender.display_name or msg.sender.open_id or "未知"
            parts.append(f"{sender_name}")

        # 2. 消息正文
        if msg.content_text:
            if parts:
                parts.append(f": {msg.content_text}")
            else:
                parts.append(msg.content_text)
        else:
            # 纯非文本消息
            if parts:
                parts.append(": [消息]")
            else:
                parts.append("[消息]")

        # 3. 回复上下文
        if _has_reply_context(msg):
            # 从 ReplyRef 中提取发送者和摘要
            reply_sender = msg.reply.sender_id or "未知"
            reply_text = msg.reply.text or "(原始内容不可用)"
            # 截断过长的引用
            if len(reply_text) > 100:
                reply_text = reply_text[:100] + "..."
            parts.append(f"\n> 回复 {reply_sender}: {reply_text}")

        # 4. 非文本资源标记
        for res in msg.resources or []:
            parts.append(f"\n[{_resource_label(res)}]")

        return "".join(parts)

    # ── meta 构造 ──

    def _build_user_meta(self, msg: InboundMessage) -> Dict[str, Any]:
        """构造用户消息的结构化 meta"""
        meta: Dict[str, Any] = {
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

        # mentions
        if msg.mentions:
            meta["mentions"] = [
                {
                    "key": m.key,
                    "name": m.name,
                    "open_id": m.open_id,
                    "is_bot": m.is_bot,
                }
                for m in msg.mentions
            ]

        # reply_to
        if _has_reply_context(msg):
            meta["reply_to"] = {
                "message_id": msg.reply.message_id,
                "sender_id": msg.reply.sender_id,
                "snippet": (msg.reply.text or "")[:100] if msg.reply.text else "",
            }
        else:
            meta["reply_to"] = None

        # resources
        if msg.resources:
            meta["resources"] = [
                {
                    "type": r.type,
                    "file_key": r.file_key,
                    "file_name": r.file_name,
                    "duration_ms": r.duration_ms,
                    "cover_image_key": r.cover_image_key,
                }
                for r in msg.resources
            ]
        else:
            meta["resources"] = []

        return meta
