"""
飞书事件 → ReAct 循环 调度器。

这是整个 Adapter 的核心编排器，负责：
1. 接收飞书事件（message、cardAction、系统事件）
2. 转换消息格式（MessageConverter）
3. 在线程池中执行 ReAct 循环
4. 通过 ReplyAdapter 将 LLM 输出发送回飞书

线程模型:
  FeishuChannel (async) → handle_message (async)
    → run_in_executor (sync thread pool): streaming_react_loop
      → on_event 回调跨线程推入 asyncio.Queue
    → consume (async): 从 Queue 读取事件 → channel.send/stream
"""

import asyncio
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
from server.lark_service.message_converter import MessageConverter
from server.lark_service.reply_adapter import ReplyAdapter


class EventHandler:
    """
    飞书事件处理器。

    绑定到 FeishuChannel，在收到事件时将消息送入 Avalon ReAct 管线，
    并将 LLM 输出发送回飞书。

    用法:
        handler = EventHandler(config)
        handler.bind(channel)
        # 随后 FeishuChannel 收到消息时会自动调用 handler 的方法
    """

    def __init__(self, config: FeishuConfig):
        self._config = config
        self._converter = MessageConverter()
        self._channel: Optional[FeishuChannel] = None

    def bind(self, channel: FeishuChannel) -> None:
        """
        注册事件回调到 FeishuChannel。

        绑定后，channel 收到事件时会自动调用对应的方法。
        """
        self._channel = channel

        channel.on(Events.MESSAGE, self._handle_message)
        channel.on(Events.CARD_ACTION, self._handle_card_action)
        channel.on(Events.BOT_ADDED, self._handle_bot_added)
        channel.on(Events.BOT_LEAVE, self._handle_bot_leave)
        channel.on(Events.REACTION, self._handle_reaction)
        channel.on(Events.REJECT, self._handle_reject)
        channel.on(Events.ERROR, self._handle_error)
        channel.on(Events.RECONNECTING, self._handle_reconnecting)
        channel.on(Events.RECONNECTED, self._handle_reconnected)

    # ════════════════════════════════════════════════════════════════
    # 消息处理（核心路径）
    # ════════════════════════════════════════════════════════════════

    async def _handle_message(self, msg: InboundMessage) -> None:
        """
        处理入站消息。

        这是整个 Adapter 最核心的路径:
          InboundMessage → session entry → ReAct 循环 → 飞书回复
        """
        # ① 转换消息
        entry = self._converter.to_session_entry(msg)
        user_input = entry["content"]
        msg_meta = entry["meta"]

        # ② 初始化回复适配器
        adapter = ReplyAdapter(
            self._channel,
            msg_meta["chat_id"],
            chat_type=msg_meta.get("chat_type", "p2p"),
            reply_to_message_id=msg_meta.get("message_id"),
            processing_reaction=self._config.processing_reaction,
            done_reaction=self._config.done_reaction,
        )

        # ③ 标记处理中（用户消息上添加 reaction）
        try:
            await adapter.mark_processing()
        except Exception:
            pass

        # ④ 创建跨线程队列
        queue: asyncio.Queue = asyncio.Queue()

        # ⑤ 启动异步消费者
        consumer = asyncio.create_task(
            adapter.consume(queue),
            name=f"feishu_reply_{msg.id}",
        )

        # ⑥ 在线程池中执行 ReAct 循环
        loop = asyncio.get_event_loop()
        try:
            await loop.run_in_executor(
                None,
                self._run_react_in_thread,
                user_input,
                entry,
                queue,
                loop,
            )
        except Exception as e:
            traceback.print_exc()
            try:
                loop.call_soon_threadsafe(
                    queue.put_nowait,
                    ("error", {"message": str(e)}),
                )
            except RuntimeError:
                pass
        finally:
            # 发送 sentinel 信号，通知消费者结束
            try:
                loop.call_soon_threadsafe(
                    queue.put_nowait,
                    ("__sentinel__", {}),
                )
            except RuntimeError:
                pass

        # 等待消费者完成
        await consumer

    def _run_react_in_thread(
            self,
            user_input: str,
            entry: Dict[str, Any],
            queue: asyncio.Queue,
            loop: asyncio.AbstractEventLoop,
        ) -> None:
        """
        在线程池中执行同步 ReAct 循环。

        这是非异步函数，运行在 ThreadPoolExecutor 中。
        通过 on_event 回调将事件跨线程推入 asyncio.Queue。
        """
        from server.services.chat_service import streaming_react_loop

        def on_event(event_type: str, data: Dict[str, Any]) -> None:
            """线程安全的跨线程事件推送"""
            try:
                loop.call_soon_threadsafe(
                    queue.put_nowait,
                    (event_type, data),
                )
            except RuntimeError:
                # 事件循环已关闭，无法推送
                pass

        # 在 history 中首条记录带上 meta
        # streaming_react_loop 内部第一条会 append user 消息，
        # 我们需要在调用后手动往 chat_history 注入 meta
        chat_history = streaming_react_loop(
            user_input,
            on_event=on_event,
            channel="feishu",
        )

        # 注入 meta 到 user 消息中
        if chat_history and len(chat_history) > 0:
            first_entry = chat_history[0]
            if first_entry.get("role") == "user":
                first_entry["meta"] = entry.get("meta", {})

        # 持久化
        from loop import session_manage

        session_manage.update_current_session(chat_history, "feishu")

        # 自动压缩检查
        session_manage.auto_compress_check_from_history(chat_history, "feishu")

    # ════════════════════════════════════════════════════════════════
    # 卡片交互
    # ════════════════════════════════════════════════════════════════

    async def _handle_card_action(self, event: CardActionEvent) -> None:
        """
        处理卡片交互事件。

        将按钮点击等交互转换为文本消息，送入 ReAct 循环处理。
        """
        entry = self._converter.card_action_to_entry(event)
        user_input = entry["content"]
        msg_meta = entry["meta"]

        adapter = ReplyAdapter(
            self._channel,
            msg_meta["chat_id"],
            chat_type=msg_meta.get("chat_type", "p2p"),
            processing_reaction=self._config.processing_reaction,
            done_reaction=self._config.done_reaction,
        )

        queue: asyncio.Queue = asyncio.Queue()
        consumer = asyncio.create_task(adapter.consume(queue))

        loop = asyncio.get_event_loop()
        try:
            await loop.run_in_executor(
                None,
                self._run_react_in_thread,
                user_input,
                entry,
                queue,
                loop,
            )
        except Exception:
            pass
        finally:
            try:
                loop.call_soon_threadsafe(queue.put_nowait, ("__sentinel__", {}))
            except RuntimeError:
                pass

        await consumer

    # ════════════════════════════════════════════════════════════════
    # 系统事件（只记录，不触发 LLM）
    # ════════════════════════════════════════════════════════════════

    async def _handle_bot_added(self, event: BotAddedEvent) -> None:
        entry = self._converter.bot_added_to_entry(event)
        self._persist_entry(entry)

    async def _handle_bot_leave(self, event: BotLeaveEvent) -> None:
        entry = self._converter.bot_leave_to_entry(event)
        self._persist_entry(entry)

    async def _handle_reaction(self, event: ReactionEvent) -> None:
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
        pass  # 可改为 print(f"[Lark] 消息被拒绝: reason={event.reason}")

    async def _handle_error(self, error: Exception) -> None:
        """处理 FeishuChannel 内部错误"""
        traceback.print_exc()

    async def _handle_reconnecting(self, *args) -> None:
        """WebSocket 正在重连"""
        pass

    async def _handle_reconnected(self, *args) -> None:
        """WebSocket 重连成功"""
        pass

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
