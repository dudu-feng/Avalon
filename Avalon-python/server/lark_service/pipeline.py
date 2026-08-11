"""
ReAct 执行管线。

封装从"用户输入"到"飞书回复发送完成"的完整生命周期：
  Queue 创建 → 线程池执行 ReAct → 事件桥接 → consumer 管理 → 会话持久化

这是 event_handler（事件路由）和 chat_service（LLM 调用）之间的胶水层。
"""

import asyncio
import logging
import time
import traceback
from typing import Any, Dict

from lark_oapi.channel import FeishuChannel

from server.lark_service.adapter import ReplyAdapter
from server.lark_service.config import FeishuConfig

logger = logging.getLogger(__name__)


class ReActPipeline:
    """
    ReAct 执行管线。

    将当前 event_handler._handle_message 中的跨线程桥接逻辑
    （Queue + 线程池 + consumer task + sentinel + 持久化）抽离为此类。

    用法:
        pipeline = ReActPipeline()
        await pipeline.execute(user_input, entry, channel, config)
    """

    # ── 公开 API ──

    async def execute(
        self,
        user_input: str,
        entry: Dict[str, Any],
        channel: FeishuChannel,
        config: FeishuConfig,
    ) -> None:
        """
        执行完整管线，阻塞直到回复发送完成。

        Args:
            user_input: 用户输入文本
            entry: MessageConverter 转换后的 session entry
            channel: 飞书 SDK Channel 实例
            config: 飞书配置
        """
        t_start = time.monotonic()
        msg_meta = entry.get("meta", {})
        msg_id = msg_meta.get("message_id", "?")
        chat_id = msg_meta.get("chat_id", "?")
        chat_type = msg_meta.get("chat_type", "p2p")

        logger.info(
            "[Lark] ═══ ReAct 管线启动 | msg_id=%s | chat_id=%s | chat_type=%s | input_len=%d",
            msg_id, chat_id, chat_type, len(user_input),
        )

        # ① 创建回复适配器
        adapter = ReplyAdapter(
            channel,
            chat_id,
            chat_type=chat_type,
            reply_to_message_id=msg_meta.get("message_id"),
            processing_reaction=config.processing_reaction,
            done_reaction=config.done_reaction,
        )
        logger.debug("[Lark] 管线: ReplyAdapter 已创建 | reaction=%s→%s",
                     config.processing_reaction, config.done_reaction)

        # ② 标记处理中（👀 reaction）
        try:
            await adapter.mark_processing()
            logger.debug("[Lark] 管线: 已标记处理中 reaction")
        except Exception:
            logger.debug("[Lark] 管线: 标记处理中 reaction 失败（可能无消息权限）")

        # ③ 创建跨线程队列
        queue: asyncio.Queue = asyncio.Queue()

        # ④ 启动异步消费者
        consumer = asyncio.create_task(
            adapter.consume(queue),
            name=f"feishu_reply_{msg_id}",
        )
        logger.debug("[Lark] 管线: 消费者任务已启动")

        # ⑤ 在线程池中执行 ReAct 循环
        loop = asyncio.get_event_loop()
        t_react_start = time.monotonic()
        try:
            logger.info("[Lark] 管线: 开始执行 ReAct 循环（线程池）...")
            await loop.run_in_executor(
                None,
                self._run_react_in_thread,
                user_input,
                entry,
                queue,
                loop,
            )
            t_react = time.monotonic() - t_react_start
            logger.info("[Lark] 管线: ReAct 循环完成 | 耗时=%.1fs", t_react)
        except Exception:
            t_react = time.monotonic() - t_react_start
            logger.error("[Lark] 管线: ReAct 循环异常 | 耗时=%.1fs", t_react)
            traceback.print_exc()
            try:
                loop.call_soon_threadsafe(
                    queue.put_nowait,
                    ("error", {"message": traceback.format_exc()[-500:]}),
                )
            except RuntimeError:
                pass
        finally:
            # ⑥ 发送 sentinel 信号，通知消费者结束
            try:
                loop.call_soon_threadsafe(
                    queue.put_nowait,
                    ("__sentinel__", {}),
                )
                logger.debug("[Lark] 管线: sentinel 信号已发送")
            except RuntimeError:
                logger.warning("[Lark] 管线: 无法发送 sentinel（事件循环已关闭）")

        # ⑦ 等待消费者完成（包括 _finalize 中的 reaction swap + send）
        await consumer
        t_total = time.monotonic() - t_start
        logger.info(
            "[Lark] ═══ ReAct 管线完成 | msg_id=%s | 总耗时=%.1fs",
            msg_id, t_total,
        )

    # ── 线程池执行体 ──

    @staticmethod
    def _run_react_in_thread(
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
        from loop import session_manage

        # 确保飞书会话已初始化（首次消息或 save_current_session 后重新分配会话 ID）
        session_manage.init_session("feishu")

        event_count = [0]  # 用列表包装以便在闭包中修改

        def on_event(event_type: str, data: Dict[str, Any]) -> None:
            """线程安全的跨线程事件推送"""
            event_count[0] += 1
            try:
                loop.call_soon_threadsafe(
                    queue.put_nowait,
                    (event_type, data),
                )
            except RuntimeError:
                # 事件循环已关闭，无法推送
                pass

        # 执行 ReAct 循环
        chat_history = streaming_react_loop(
            user_input,
            on_event=on_event,
            channel="feishu",
        )

        logger.debug(
            "[Lark] ReAct 线程: streaming_react_loop 返回 | 历史条目=%d | 事件推送=%d",
            len(chat_history) if chat_history else 0,
            event_count[0],
        )

        # 注入 meta 到 user 消息
        if chat_history and len(chat_history) > 0:
            first_entry = chat_history[0]
            if first_entry.get("role") == "user":
                first_entry["meta"] = entry.get("meta", {})

        # 持久化
        session_manage.update_current_session(chat_history, "feishu")
        logger.debug("[Lark] ReAct 线程: 会话已持久化")

        # 自动压缩检查
        session_manage.auto_compress_check_from_history(chat_history, "feishu")
