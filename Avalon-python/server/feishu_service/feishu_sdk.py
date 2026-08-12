"""
独立飞书 SDK 服务层。

职责:
  1. 维护与飞书的 WebSocket 长连接（生命周期管理）
  2. 把收到的飞书事件分发给所有订阅者（EventBus 多播）
  3. 提供向飞书发送消息 / 操作表情的 API

不关心:
  - 消息格式如何转换（MessageConverter 的事）
  - LLM 如何调用、ReAct 如何循环（ReActPipeline 的事）
  - 会话如何持久化（session_manage 的事）

使用示例:
    # ① 获取单例
    sdk = get_sdk()

    # ② 注册业务消费者
    sdk.on(FeishuEvent.MESSAGE, my_handler)
    sdk.on(FeishuEvent.ERROR, my_logger)

    # ③ 启动
    await sdk.start()

    # ④ 发送消息
    await sdk.send_message("oc_xxx", {"markdown": "你好"})

    # ⑤ 关闭
    await sdk.stop()
"""

import asyncio
import inspect
import warnings
from enum import StrEnum
from typing import Any, Callable, Optional

# 飞书 SDK 的 protobuf 模块使用了已弃用的 pkg_resources，
# 在我们的代码层面无法修复，屏蔽该警告以避免终端噪音。
warnings.filterwarnings("ignore", message="pkg_resources is deprecated as an API")

from lark_oapi import LogLevel
from lark_oapi.channel import (
    FeishuChannel,
    PolicyConfig,
    SafetyConfig,
    DedupConfig,
    OutboundConfig,
    RetryConfig,
    InboundMessage,
    CardActionEvent,
    BotAddedEvent,
    BotLeaveEvent,
    ReactionEvent,
    RejectEvent,
    Events,
)

from server.lark_service.config import FeishuConfig


# ============================================================
#  事件类型枚举
# ============================================================

class FeishuEvent(StrEnum):
    """SDK 服务对外暴露的飞书事件类型"""
    MESSAGE       = "message"        # InboundMessage — 收到用户消息
    CARD_ACTION   = "card_action"    # CardActionEvent — 卡片按钮点击
    BOT_ADDED     = "bot_added"      # BotAddedEvent — 机器人被拉入群聊
    BOT_LEAVE     = "bot_leave"      # BotLeaveEvent — 机器人被移出群聊
    REACTION      = "reaction"       # ReactionEvent — 用户添加/移除表情
    REJECT        = "reject"         # RejectEvent — 消息被策略层丢弃
    ERROR         = "error"          # Exception — SDK 内部错误
    RECONNECTING  = "reconnecting"   # None — 正在重连
    RECONNECTED   = "reconnected"    # None — 重连成功
    DISCONNECTED  = "disconnected"   # None — 连接已断开


# ============================================================
#  轻量级事件总线
# ============================================================

class EventBus:
    """
    类型化事件总线。

    特点:
      - 一个事件类型 → 多个订阅者（多播）
      - 单个订阅者异常不影响其他订阅者（异常隔离）
      - 自动检测回调是 async 还是 sync
    """

    def __init__(self):
        self._subscribers: dict[FeishuEvent, list[Callable]] = {
            e: [] for e in FeishuEvent
        }

    def subscribe(self, event: FeishuEvent, callback: Callable) -> None:
        """注册事件订阅者"""
        if callback not in self._subscribers[event]:
            self._subscribers[event].append(callback)

    def unsubscribe(self, event: FeishuEvent, callback: Callable) -> None:
        """移除事件订阅者"""
        try:
            self._subscribers[event].remove(callback)
        except ValueError:
            pass

    async def dispatch(self, event: FeishuEvent, data: Any) -> None:
        """
        向所有订阅者分发事件。

        单个订阅者抛异常 → 捕获后继续通知下一个。
        保证日志/监控类订阅者不影响消息处理主流程。
        """
        for cb in self._subscribers.get(event, []):
            try:
                if inspect.iscoroutinefunction(cb):
                    await cb(data)
                else:
                    cb(data)
            except Exception:
                pass


# ============================================================
#  独立飞书 SDK 服务
# ============================================================

class FeishuSDKService:
    """
    独立飞书 SDK 服务。

    封装与飞书开放平台的全部通信细节，对外暴露:
      - 事件订阅接口 (on/off)
      - 消息发送接口 (send_message/add_reaction/remove_reaction)
      - 生命周期接口 (start/stop/is_connected)

    设计原则:
      - 零业务逻辑: 不持有 Converter / Pipeline / Session 引用
      - 事件源模式: 只负责广播飞书事件，不关心消费者是谁
      - 单例: 全局只有一个 SDK 连接
    """

    def __init__(self, config: FeishuConfig):
        if not config.is_configured():
            raise ValueError("飞书凭据未配置，请在 .env 中设置 LARK_APP_ID 和 LARK_APP_SECRET")

        self._config = config
        self._sdk_channel: Optional[FeishuChannel] = None
        self._event_bus = EventBus()
        self._shutting_down = False

    # ── 事件订阅 ────────────────────────────────────────────

    def on(self, event: FeishuEvent, callback: Callable) -> "FeishuSDKService":
        """
        订阅飞书事件。返回 self 以支持链式调用。

        用法:
            sdk.on(FeishuEvent.MESSAGE, handle_msg)
            sdk.on(FeishuEvent.ERROR, log_error).on(FeishuEvent.RECONNECTING, on_reconnect)
        """
        self._event_bus.subscribe(event, callback)
        return self

    def off(self, event: FeishuEvent, callback: Callable) -> "FeishuSDKService":
        """移除事件订阅。返回 self 以支持链式调用。"""
        self._event_bus.unsubscribe(event, callback)
        return self

    # ── 生命周期 ────────────────────────────────────────────

    async def start(self) -> None:
        """
        创建 SDK Channel 并建立 WebSocket 连接。

        流程:
          ① 根据 config 创建 FeishuChannel 实例
          ② 注册内部事件转发器（SDK 回调 → EventBus 多播）
          ③ 补注册 SDK 遗漏的事件处理器
          ④ 启动连接（WS 模式通过 connect_until_ready 等待就绪）
        """
        # ① 创建 SDK Channel
        self._sdk_channel = FeishuChannel(
            app_id=self._config.app_id,
            app_secret=self._config.app_secret,
            domain=self._config.domain if self._config.domain else None,
            transport=self._config.transport,
            encrypt_key=self._config.encrypt_key if self._config.encrypt_key else None,
            verification_token=(
                self._config.verification_token
                if self._config.verification_token
                else None
            ),
            log_level=LogLevel[self._config.log_level] if self._config.log_level else LogLevel.INFO,
            policy=PolicyConfig(
                dm_policy="open" if self._config.dm_enabled else "disabled",
                group_policy="open" if self._config.group_enabled else "disabled",
                require_mention=self._config.require_mention,
                respond_to_mention_all=self._config.respond_to_mention_all,
            ),
            safety=SafetyConfig(
                dedup=DedupConfig(
                    ttl_seconds=self._config.dedup_ttl_hours * 3600,
                ),
            ),
            outbound=OutboundConfig(
                retry=RetryConfig(max_attempts=3),
            ),
        )

        # ② 注册内部事件转发（SDK 原生回调 → EventBus.dispatch）
        self._sdk_channel.on(Events.MESSAGE, self._on_message)
        self._sdk_channel.on(Events.CARD_ACTION, self._on_card_action)
        self._sdk_channel.on(Events.BOT_ADDED, self._on_bot_added)
        self._sdk_channel.on(Events.BOT_LEAVE, self._on_bot_leave)
        self._sdk_channel.on(Events.REACTION, self._on_reaction)
        self._sdk_channel.on(Events.REJECT, self._on_reject)
        self._sdk_channel.on(Events.ERROR, self._on_error)
        self._sdk_channel.on(Events.RECONNECTING, self._on_reconnecting)
        self._sdk_channel.on(Events.RECONNECTED, self._on_reconnected)

        # ③ 补注册 SDK 遗漏的事件处理器
        self._patch_missing_processors()

        # ④ 启动连接
        if self._config.transport == "ws":
            # WebSocket 模式
            # 问题：lark_oapi.ws.client 在模块导入时执行了
            #   loop = asyncio.get_event_loop()
            # 此时 FastAPI event loop 已存在，loop 被固定为运行中的 loop，
            # 导致 connect_until_ready 内部 ws_client.start() 调
            # run_until_complete() 时报 "already running"。
            # 修复：在启动前把 SDK 模块里的 loop 替换为新的未运行 loop。
            import lark_oapi.ws.client as _ws_client

            _ws_client.loop = asyncio.new_event_loop()
            await self._sdk_channel.connect_until_ready(timeout=30)
        else:
            # Webhook 模式：start() 初始化 dispatcher 后返回
            self._sdk_channel.start()

    async def stop(self) -> None:
        """
        优雅关闭 SDK 连接。

        流程:
          ① 标记关闭中（阻止重连/错误事件的噪音日志）
          ② 解绑内部事件转发器
          ③ 断开 WebSocket 连接
          ④ 等待 SDK 内部后台 task 清理
          ⑤ 广播 DISCONNECTED 事件
        """
        self._shutting_down = True

        if self._sdk_channel is not None:
            # ② 解绑内部回调（SDK 的 on() 没有 off()，替换为空回调即可）
            self._sdk_channel.on(Events.MESSAGE, lambda _: None)
            self._sdk_channel.on(Events.CARD_ACTION, lambda _: None)
            self._sdk_channel.on(Events.BOT_ADDED, lambda _: None)
            self._sdk_channel.on(Events.BOT_LEAVE, lambda _: None)
            self._sdk_channel.on(Events.REACTION, lambda _: None)
            self._sdk_channel.on(Events.REJECT, lambda _: None)
            self._sdk_channel.on(Events.ERROR, lambda _: None)
            self._sdk_channel.on(Events.RECONNECTING, lambda _: None)
            self._sdk_channel.on(Events.RECONNECTED, lambda _: None)

            # ③ 断开连接
            try:
                await self._sdk_channel.disconnect()
            except Exception:
                pass

            # ④ 给 SDK 内部后台任务（_ping_loop、_receive_message_loop、
            # _start_clear_cron）一点时间响应关闭信号并正常退出，
            # 避免 asyncio 报 "Task was destroyed but it is pending!"
            try:
                await asyncio.sleep(0.5)
            except asyncio.CancelledError:
                pass

            self._sdk_channel = None

        # ⑤ 广播断开事件
        await self._event_bus.dispatch(FeishuEvent.DISCONNECTED, None)

    @property
    def is_connected(self) -> bool:
        """当前 SDK 连接是否已建立"""
        return self._sdk_channel is not None

    # ── 消息发送 API ────────────────────────────────────────

    async def send_message(
          self,
          chat_id: str,
          content: dict,
          *,
          reply_to: Optional[str] = None,
      ):
        """
        向指定会话发送消息。

        Args:
            chat_id: 飞书会话 ID（oc_xxx）
            content: 消息内容，{"text": "..."} 或 {"markdown": "..."}
            reply_to: 被引用回复的消息 ID（可选）

        Returns:
            SDK 返回的 SendResult

        Raises:
            RuntimeError: SDK 服务未启动
        """
        if self._sdk_channel is None:
            raise RuntimeError("SDK 服务未启动，请先调用 start()")

        opts: dict = {}
        if reply_to:
            opts["reply_to"] = reply_to

        return await self._sdk_channel.send(chat_id, content, opts)

    async def add_reaction(self, message_id: str, emoji: str):
        """
        对消息添加表情回应。

        Returns:
            SDK 返回的结果对象，包含 result.success 和 result.raw
        """
        if self._sdk_channel is None:
            raise RuntimeError("SDK 服务未启动，请先调用 start()")

        return await self._sdk_channel.add_reaction(message_id, emoji)

    async def remove_reaction(self, message_id: str, reaction_id: str) -> None:
        """移除消息上的指定表情回应"""
        if self._sdk_channel is None:
            raise RuntimeError("SDK 服务未启动，请先调用 start()")

        await self._sdk_channel.remove_reaction(message_id, reaction_id)

    # ── 内部：事件转发器 ─────────────────────────────────────

    async def _on_message(self, msg: InboundMessage) -> None:
        await self._event_bus.dispatch(FeishuEvent.MESSAGE, msg)

    async def _on_card_action(self, event: CardActionEvent) -> None:
        await self._event_bus.dispatch(FeishuEvent.CARD_ACTION, event)

    async def _on_bot_added(self, event: BotAddedEvent) -> None:
        await self._event_bus.dispatch(FeishuEvent.BOT_ADDED, event)

    async def _on_bot_leave(self, event: BotLeaveEvent) -> None:
        await self._event_bus.dispatch(FeishuEvent.BOT_LEAVE, event)

    async def _on_reaction(self, event: ReactionEvent) -> None:
        await self._event_bus.dispatch(FeishuEvent.REACTION, event)

    async def _on_reject(self, event: RejectEvent) -> None:
        """消息被 SDK 策略/安全层丢弃（如去重、不在白名单），属正常行为"""
        await self._event_bus.dispatch(FeishuEvent.REJECT, event)

    async def _on_error(self, error: Exception) -> None:
        if not self._shutting_down:
            await self._event_bus.dispatch(FeishuEvent.ERROR, error)

    def _on_reconnecting(self, *_args) -> None:
        """
        WebSocket 正在重连。

        注意: SDK 以同步方式调用此回调，所以本方法必须保持同步。
        通过 create_task 将事件异步分发给订阅者。
        """
        if not self._shutting_down:
            asyncio.create_task(
                self._event_bus.dispatch(FeishuEvent.RECONNECTING, None)
            )

    def _on_reconnected(self, *_args) -> None:
        """
        WebSocket 重连成功。

        注意: SDK 以同步方式调用此回调，所以本方法必须保持同步。
        """
        if not self._shutting_down:
            asyncio.create_task(
                self._event_bus.dispatch(FeishuEvent.RECONNECTED, None)
            )

    # ── 内部：补注册 SDK 遗漏的事件处理器 ──────────────────────

    def _patch_missing_processors(self) -> None:
        """
        为 SDK 内部 _build_dispatcher() 遗漏的事件类型注册空处理器。

        飞书 SDK 生成的代码中包含以下事件处理器类，
        但 FeishuChannel._build_dispatcher() 未调用对应的 register_* 方法，
        导致收到这些事件时 SDK 打印 ERROR 级别的 "processor not found"。

        这些事件是飞书的系统通知，不需要机器人响应，
        注册空处理器即可消除日志噪音。
        """
        from lark_oapi.api.im.v1.processor import (
            P2ImChatAccessEventBotP2pChatEnteredV1Processor,
        )

        dispatcher = self._sdk_channel.dispatcher

        # 用户打开机器人私聊会话（im.chat.access_event.bot_p2p_chat_entered_v1）
        key = "p2.im.chat.access_event.bot_p2p_chat_entered_v1"
        if key not in dispatcher._processorMap:
            dispatcher._processorMap[key] = (
                P2ImChatAccessEventBotP2pChatEnteredV1Processor(
                    lambda data: None
                )
            )


# ============================================================
#  单例管理
# ============================================================

_sdk_instance: Optional[FeishuSDKService] = None
_instance_lock = asyncio.Lock()


def get_sdk() -> Optional[FeishuSDKService]:
    """
    获取当前 SDK 服务单例。

    未初始化时返回 None。不会自动创建——初始化需显式调用 create_sdk() 或 startup_sdk()。
    """
    return _sdk_instance


async def create_sdk(config: Optional[FeishuConfig] = None) -> FeishuSDKService:
    """
    创建并启动 SDK 服务单例。

    如果单例已存在且处于连接状态，直接返回现有实例（幂等）。
    如果单例已存在但连接已断开，先销毁旧实例再创建新实例。

    Args:
        config: 飞书配置。如果为 None，自动从 .env 读取。

    Returns:
        FeishuSDKService 单例

    Raises:
        ValueError: 凭据未配置
    """
    global _sdk_instance

    async with _instance_lock:
        # 已存在且连接中 → 幂等返回
        if _sdk_instance is not None and _sdk_instance.is_connected:
            return _sdk_instance

        # 已存在但连接断开 → 先清理
        if _sdk_instance is not None:
            await _sdk_instance.stop()
            _sdk_instance = None

        # 创建新实例
        if config is None:
            config = FeishuConfig.from_env()

        service = FeishuSDKService(config)
        await service.start()
        _sdk_instance = service
        return service


async def destroy_sdk() -> None:
    """
    停止并销毁 SDK 服务单例。

    如果单例不存在或已销毁，此调用是安全的（幂等）。
    """
    global _sdk_instance

    async with _instance_lock:
        if _sdk_instance is not None:
            await _sdk_instance.stop()
            _sdk_instance = None


# ============================================================
#  公开导出
# ============================================================

__all__ = [
    "FeishuEvent",
    "EventBus",
    "FeishuSDKService",
    "get_sdk",
    "create_sdk",
    "destroy_sdk",
]
