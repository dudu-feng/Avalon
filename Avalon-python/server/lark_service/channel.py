"""
FeishuChannel SDK 生命周期管理。

仅负责：
- 根据配置创建 SDK FeishuChannel 实例
- 启动 / 停止连接
- 健康检查

不负责：事件处理器注册、FastAPI lifespan 集成（这些由 LarkService 统一管理）。
"""

import asyncio
import warnings
from typing import Optional

# 飞书 SDK 的 protobuf 模块使用了已弃用的 pkg_resources，
# 在我们的代码层面无法修复，屏蔽该警告以避免终端噪音。
warnings.filterwarnings(
    "ignore", message="pkg_resources is deprecated as an API"
)

from lark_oapi import LogLevel
from lark_oapi.channel import (
    FeishuChannel,
    PolicyConfig,
    SafetyConfig,
    DedupConfig,
    OutboundConfig,
    RetryConfig,
)

from server.lark_service.config import FeishuConfig


class LarkChannel:
    """
    SDK FeishuChannel 的纯生命周期管理器。

    不创建 EventHandler，不绑定事件回调。
    事件绑定由 LarkService 在 start() 流程中统一装配。

    用法:
        config = FeishuConfig.from_env()
        channel = LarkChannel(config)
        await channel.start()
        # ... handler.bind(channel.sdk_channel) ...
        await channel.stop()
    """

    def __init__(self, config: FeishuConfig):
        self._config = config
        self._sdk_channel: Optional[FeishuChannel] = None

    @property
    def sdk_channel(self) -> Optional[FeishuChannel]:
        """当前的 SDK FeishuChannel 实例（启动后可用，供 handler.bind() 使用）"""
        return self._sdk_channel

    async def start(self) -> None:
        """
        创建 SDK FeishuChannel 并启动连接。

        Raises:
            ValueError: 凭据未配置
            ConnectionError: 无法连接到飞书
        """
        if not self._config.is_configured():
            raise ValueError("飞书凭据未配置，请在 .env 中设置 LARK_APP_ID 和 LARK_APP_SECRET")

        # 创建 SDK Channel
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
            # 策略配置
            policy=PolicyConfig(
                dm_policy="open" if self._config.dm_enabled else "disabled",
                group_policy="open" if self._config.group_enabled else "disabled",
                require_mention=self._config.require_mention,
                respond_to_mention_all=self._config.respond_to_mention_all,
            ),
            # 安全配置
            safety=SafetyConfig(
                dedup=DedupConfig(
                    ttl_seconds=self._config.dedup_ttl_hours * 3600,
                ),
            ),
            # 出站配置（重试）
            outbound=OutboundConfig(
                retry=RetryConfig(max_attempts=3),
            ),
        )

        # 启动长连接
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
            # connect_until_ready 在线程池运行 channel.start()，
            # 仅等待 WS 连接就绪即返回，不阻塞在 _select() 死循环上
            await self._sdk_channel.connect_until_ready(timeout=30)
        else:
            # Webhook 模式：start() 初始化 dispatcher 后返回
            self._sdk_channel.start()

        # 补注册 SDK 遗漏的事件处理器
        self._patch_missing_processors()

    def _patch_missing_processors(self) -> None:
        """
        为 SDK 内部 _build_dispatcher() 遗漏的事件类型注册空处理器。

        飞书 SDK 生成的代码中包含以下事件处理器类，
        但 FeishuChannel._build_dispatcher() 未调用对应的 register_* 方法，
        导致收到这些事件时 SDK 打印 ERROR 级别的 "processor not found"。

        这些事件是飞书的系统通知，不需要机器人响应，
        注册空处理器即可消除日志噪音。后续如需响应（如私聊欢迎语），
        可在此替换为真实回调。
        """
        from lark_oapi.api.im.v1.processor import (
            P2ImChatAccessEventBotP2pChatEnteredV1Processor,
        )

        dispatcher = self._sdk_channel.dispatcher
        patched_count = 0

        # 用户打开机器人私聊会话（im.chat.access_event.bot_p2p_chat_entered_v1）
        if "p2.im.chat.access_event.bot_p2p_chat_entered_v1" not in dispatcher._processorMap:
            dispatcher._processorMap[
                "p2.im.chat.access_event.bot_p2p_chat_entered_v1"
            ] = P2ImChatAccessEventBotP2pChatEnteredV1Processor(
                lambda data: None  # 空处理，后续可扩展为私聊欢迎语等逻辑
            )
            patched_count += 1

        if patched_count > 0:
            pass

    async def stop(self) -> None:
        """断开 SDK Channel 连接"""
        if self._sdk_channel is not None:
            # 断开 WebSocket 连接
            try:
                await self._sdk_channel.disconnect()
            except Exception:
                pass

            # 给 SDK 内部后台任务（_ping_loop、_receive_message_loop、
            # _start_clear_cron）一点时间响应关闭信号并正常退出，
            # 避免 asyncio 报 "Task was destroyed but it is pending!"
            try:
                await asyncio.sleep(0.5)
            except asyncio.CancelledError:
                pass

            self._sdk_channel = None

    async def health_check(self) -> bool:
        """检查飞书渠道是否正常运行"""
        return self._sdk_channel is not None
