"""
飞书事件 → ReAct 循环 调度器。

职责（精简后）：
1. 接收飞书事件（message、cardAction、系统事件）
2. 转换消息格式（MessageConverter）
3. 委托给 ReActPipeline 执行 LLM 管线
4. 系统事件持久化

不再负责：线程池管理、Queue 桥接、consumer 管理（已移至 pipeline.py）
"""

import logging
import traceback
from typing import Dict, Any, Optional

from lark_oapi.channel import (
    FeishuChannel,
    InboundMessage,
    CardActionEvent,
    BotAddedEvent,
    BotLeaveEvent,
    ReactionEvent,
    MessageReadEvent,
    CommentEvent,
    RejectEvent,
    Events,
)

from server.lark_service.config import FeishuConfig
from server.lark_service.converter import MessageConverter
from server.lark_service.pipeline import ReActPipeline

logger = logging.getLogger(__name__)


class EventHandler:
    """
    飞书事件处理器。

    绑定到 FeishuChannel，在收到事件时：
    - 消息/卡片事件 → converter → pipeline.execute()
    - 系统事件 → converter → persist

    用法:
        handler = EventHandler(config, pipeline)
        handler.bind(channel.sdk_channel)
        # 随后 FeishuChannel 收到消息时会自动调用 handler 的方法
    """

    def __init__(self, config: FeishuConfig, pipeline: ReActPipeline):
        self._config = config
        self._converter = MessageConverter()
        self._pipeline = pipeline
        self._channel: Optional[FeishuChannel] = None
        self._shutting_down: bool = False

    def bind(self, channel: FeishuChannel) -> None:
        """
        注册事件回调到 FeishuChannel。

        绑定后，channel 收到事件时会自动调用对应的方法。
        """
        self._channel = channel
        self._shutting_down = False

        channel.on(Events.MESSAGE, self._handle_message)
        channel.on(Events.CARD_ACTION, self._handle_card_action)
        channel.on(Events.BOT_ADDED, self._handle_bot_added)
        channel.on(Events.BOT_LEAVE, self._handle_bot_leave)
        channel.on(Events.REACTION, self._handle_reaction)
        channel.on(Events.REJECT, self._handle_reject)
        channel.on(Events.ERROR, self._handle_error)
        channel.on(Events.RECONNECTING, self._handle_reconnecting)
        channel.on(Events.RECONNECTED, self._handle_reconnected)

        logger.info("[Lark] EventHandler 已绑定 %d 个事件回调", 9)

    def unbind(self, channel: FeishuChannel) -> None:
        """
        解除事件回调绑定。

        在 LarkService.stop() 中先调用此方法，
        防止 disconnect 过程中触发重连/错误回调导致警告。
        """
        self._shutting_down = True
        self._channel = None
        logger.info("[Lark] EventHandler 已解绑所有事件回调")

    # ════════════════════════════════════════════════════════════════
    # 消息处理（核心路径）
    # ════════════════════════════════════════════════════════════════

    async def _handle_message(self, msg: InboundMessage) -> None:
        """
        处理入站消息。

        核心路径:
          InboundMessage → session entry → ReActPipeline → 飞书回复
        """
        sender = msg.sender.display_name or msg.sender.open_id or "未知"
        chat_type = msg.conversation.chat_type
        content_preview = (msg.content_text or "[非文本]")[:50]

        logger.info(
            "[Lark] >>> 收到消息 | 发送者=%s | chat_type=%s | chat_id=%s | msg_id=%s | 内容=%s",
            sender, chat_type, msg.conversation.chat_id, msg.id, content_preview,
        )
        logger.debug(
            "[Lark] 消息详情 | raw_content_type=%s | mentions=%d | resources=%d | reply=%s",
            msg.raw_content_type,
            len(msg.mentions) if msg.mentions else 0,
            len(msg.resources) if msg.resources else 0,
            msg.reply.message_id if msg.reply else None,
        )

        entry = self._converter.to_session_entry(msg)
        logger.debug("[Lark] 消息已转换 | entry_content=%s", entry["content"][:80])

        await self._pipeline.execute(
            user_input=entry["content"],
            entry=entry,
            channel=self._channel,
            config=self._config,
        )
        logger.info("[Lark] <<< 消息处理完成 | msg_id=%s", msg.id)

    # ════════════════════════════════════════════════════════════════
    # 卡片交互
    # ════════════════════════════════════════════════════════════════

    async def _handle_card_action(self, event: CardActionEvent) -> None:
        """
        处理卡片交互事件。

        将按钮点击等交互转换为文本消息，送入 ReAct 管线处理。
        """
        logger.info(
            "[Lark] >>> 收到卡片交互 | 操作者=%s | tag=%s | option=%s",
            event.operator.name or "未知",
            event.action.tag,
            event.action.option,
        )
        entry = self._converter.card_action_to_entry(event)
        await self._pipeline.execute(
            user_input=entry["content"],
            entry=entry,
            channel=self._channel,
            config=self._config,
        )
        logger.info("[Lark] <<< 卡片交互处理完成")

    # ════════════════════════════════════════════════════════════════
    # 系统事件（只记录，不触发 LLM）
    # ════════════════════════════════════════════════════════════════

    async def _handle_bot_added(self, event: BotAddedEvent) -> None:
        logger.info(
            "[Lark] >>> 机器人被加入群聊 | chat_name=%s | 操作者=%s",
            event.chat_name, event.operator.name or "未知",
        )
        entry = self._converter.bot_added_to_entry(event)
        self._persist_entry(entry)

    async def _handle_bot_leave(self, event: BotLeaveEvent) -> None:
        logger.info(
            "[Lark] >>> 机器人被移出群聊 | chat_id=%s | 操作者=%s",
            event.chat_id, event.operator.name or "系统",
        )
        entry = self._converter.bot_leave_to_entry(event)
        self._persist_entry(entry)

    async def _handle_reaction(self, event: ReactionEvent) -> None:
        logger.debug(
            "[Lark] >>> 收到 reaction | emoji=%s | msg_id=%s",
            event.emoji_type, event.message_id,
        )
        entry = self._converter.reaction_to_entry(event)
        if entry:
            self._persist_entry(entry)

    # ════════════════════════════════════════════════════════════════
    # 策略 / 错误 / 连接事件
    # ════════════════════════════════════════════════════════════════

    async def _handle_reject(self, event: RejectEvent) -> None:
        """
        处理被策略/安全管线拒绝的消息。

        这些消息被 SDK 的策略层丢弃（如重复消息、不在白名单的群），
        属于正常行为，仅记录日志即可。
        """
        logger.debug("[Lark] 消息被策略拒绝 | reason=%s", getattr(event, "reason", "未知"))

    async def _handle_error(self, error: Exception) -> None:
        """处理 FeishuChannel 内部错误"""
        if not self._shutting_down:
            logger.error("[Lark] FeishuChannel 内部错误: %s", error)
            traceback.print_exc()

    def _handle_reconnecting(self, *args) -> None:
        """WebSocket 正在重连（SDK 以同步方式调用，必须保持同步）"""
        logger.warning("[Lark] WebSocket 正在重连...")

    def _handle_reconnected(self, *args) -> None:
        """WebSocket 重连成功（SDK 以同步方式调用，必须保持同步）"""
        logger.info("[Lark] WebSocket 重连成功")

    # ════════════════════════════════════════════════════════════════
    # 内部辅助
    # ════════════════════════════════════════════════════════════════

    def _persist_entry(self, entry: Dict[str, Any]) -> None:
        """将单条消息持久化到当前飞书会话"""
        try:
            from loop import session_manage
            session_manage.update_current_session([entry], "feishu")
        except Exception:
            pass
