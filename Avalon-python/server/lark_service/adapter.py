"""
ReAct 事件 → 飞书回复 适配器。

消费 ReAct 循环通过 on_event 回调产出的事件流，将其转换为飞书消息发送。

Phase 1（当前实现）:
  - 在用户消息上添加 reaction 表情标记（处理中 → 完成）
  - 累积所有 chat_message delta
  - 在 done 时一次性发送完整 markdown
  - 私聊不引用回复

Phase 2（后续升级）:
  - 利用 CardKit 流式卡片，逐 token 更新
  - 使用 channel.create_card_instance() + update_card_element_content()
"""

import asyncio
from typing import Any, Dict, Optional

from lark_oapi.channel import FeishuChannel


class ReplyAdapter:
    """
    ReAct 事件消费者，将 LLM 输出转换为飞书回复。

    用法:
        adapter = ReplyAdapter(
            channel, chat_id,
            chat_type="group",
            reply_to_message_id="om_xxx",
            processing_reaction="👀",
            done_reaction="✅",
        )
        await adapter.mark_processing()

        consumer = asyncio.create_task(adapter.consume(queue))
        # ... ReAct 循环在线程池中运行，on_event 推入 queue ...
        await consumer
    """

    def __init__(
        self,
        channel: FeishuChannel,
        chat_id: str,
        chat_type: str = "p2p",
        reply_to_message_id: Optional[str] = None,
        processing_reaction: str = "👀",
        done_reaction: str = "✅",
    ):
        self._channel = channel
        self._chat_id = chat_id
        self._chat_type = chat_type
        self._reply_to_message_id = reply_to_message_id
        self._processing_reaction = processing_reaction
        self._done_reaction = done_reaction

        # Phase 1 累积缓冲区
        self._accumulated_text: str = ""
        self._has_error: bool = False
        self._error_message: str = ""
        self._finalized: bool = False

        # 处理中 reaction 的 reaction_id（用于后续移除）
        self._processing_reaction_id: Optional[str] = None

    # ── 处理中标记 ──

    async def mark_processing(self) -> None:
        """在用户消息上添加 reaction 表情标记，表示正在处理"""
        if self._reply_to_message_id:
            try:
                result = await self._channel.add_reaction(
                    self._reply_to_message_id, self._processing_reaction
                )
                # 从 API 响应中提取 reaction_id（兼容 camelCase / snake_case）
                if result.success and result.raw:
                    data = result.raw.get("data", {})
                    self._processing_reaction_id = data.get(
                        "reaction_id"
                    ) or data.get("reactionId", "")
            except Exception:
                pass

    # ── 事件消费主循环 ──

    async def consume(self, queue: asyncio.Queue) -> None:
        """
        消费 ReAct 事件队列，转换为飞书消息发送。

        识别的 event_type:
          chat_message   → 累积 delta
          done           → 触发 finalize
          error          → 记录错误
          __sentinel__   → 队列结束标记，触发 finalize
        """
        event_stats: Dict[str, int] = {}
        while True:
            event_type, data = await queue.get()

            if event_type == "__sentinel__":
                await self._finalize()
                break

            event_stats[event_type] = event_stats.get(event_type, 0) + 1
            handler = getattr(self, f"_on_{event_type}", None)
            if handler:
                try:
                    await handler(data)
                except Exception:
                    pass

    # ── 事件处理器 ──

    async def _on_chat_message(self, data: Dict[str, Any]) -> None:
        delta = data.get("delta", "")
        self._accumulated_text += delta
        # 每累积 200 字符打印一次进度

    async def _on_error(self, data: Dict[str, Any]) -> None:
        self._has_error = True
        self._error_message = data.get("message", "处理出错")

    async def _on_done(self, data: Dict[str, Any]) -> None:
        await self._finalize()

    # 以下事件不向用户展示，仅占位
    async def _on_chat_thought(self, data: Dict[str, Any]) -> None:
        pass

    async def _on_chat_start(self, data: Dict[str, Any]) -> None:
        pass

    async def _on_chat_stop(self, data: Dict[str, Any]) -> None:
        pass

    async def _on_action_start(self, data: Dict[str, Any]) -> None:
        pass

    async def _on_action_step(self, data: Dict[str, Any]) -> None:
        pass

    async def _on_action_tool_call(self, data: Dict[str, Any]) -> None:
        pass

    async def _on_action_tool_result(self, data: Dict[str, Any]) -> None:
        pass

    async def _on_action_sub_analysis(self, data: Dict[str, Any]) -> None:
        pass

    async def _on_action_finished(self, data: Dict[str, Any]) -> None:
        pass

    # ── 最终回复 ──

    async def _finalize(self) -> None:
        """
        发送最终回复。

        策略：
          1. 标记 done reaction
          2. 有累积文本 → 发送 markdown 回复（群聊引用，私聊不引用）
          3. 有错误 → 发送错误通知
        """
        if self._finalized:
            return
        self._finalized = True

        reply_len = len(self._accumulated_text)

        # ① 取消处理中 reaction → 添加完成 reaction
        if self._reply_to_message_id:
            # 先移除处理中标记
            if self._processing_reaction_id:
                try:
                    await self._channel.remove_reaction(
                        self._reply_to_message_id, self._processing_reaction_id
                    )
                except Exception:
                    pass
            # 再添加完成标记
            try:
                await self._channel.add_reaction(
                    self._reply_to_message_id, self._done_reaction
                )
            except Exception:
                pass

        # ② 发送回复
        if self._accumulated_text:
            opts = {}
            # 私聊不引用消息；群聊/话题引用原消息
            if self._chat_type != "p2p" and self._reply_to_message_id:
                opts["reply_to"] = self._reply_to_message_id

            await self._channel.send(
                self._chat_id,
                {"markdown": self._accumulated_text},
                opts,
            )

        elif self._has_error:
            await self._channel.send(
                self._chat_id,
                {"text": f"处理出错: {self._error_message}"},
            )
