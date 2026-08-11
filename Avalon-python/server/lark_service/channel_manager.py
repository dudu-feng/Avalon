"""
FeishuChannel 生命周期管理。

负责：
- 根据配置创建 FeishuChannel 实例
- 注册事件处理器
- 启动 / 停止连接
- 健康检查
"""

import asyncio
import logging
from typing import Optional

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
from server.lark_service.event_handler import EventHandler

logger = logging.getLogger(__name__)


class ChannelManager:
    """
    FeishuChannel 的生命周期管理器。

    在 FastAPI lifespan 中由 create_app() 创建和销毁。

    用法:
        config = FeishuConfig.from_env()
        manager = ChannelManager(config)
        await manager.start()
        # ... 应用运行中 ...
        await manager.stop()
    """

    def __init__(self, config: FeishuConfig):
        self._config = config
        self._channel: Optional[FeishuChannel] = None
        self._handler: Optional[EventHandler] = None

    @property
    def channel(self) -> Optional[FeishuChannel]:
        """当前的 FeishuChannel 实例（启动后可用）"""
        return self._channel

    @property
    def handler(self) -> Optional[EventHandler]:
        """当前的事件处理器（启动后可用）"""
        return self._handler

    async def start(self) -> None:
        """
        创建并启动 FeishuChannel。

        Raises:
            ValueError: 凭据未配置
            ConnectionError: 无法连接到飞书
        """
        if not self._config.is_configured():
            raise ValueError("飞书凭据未配置，请在 .env 中设置 LARK_APP_ID 和 LARK_APP_SECRET")

        logger.info("[Lark] 正在启动飞书渠道...")

        # 创建 Channel
        self._channel = FeishuChannel(
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

        # 注册事件处理器
        self._handler = EventHandler(self._config)
        self._handler.bind(self._channel)

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
            await self._channel.connect_until_ready(timeout=30)
            logger.info("[Lark] 飞书渠道已启动 (WebSocket 长连接)")
        else:
            # Webhook 模式：start() 初始化 dispatcher 后返回
            self._channel.start()
            logger.info("[Lark] 飞书渠道已启动 (Webhook 模式)")

    async def stop(self) -> None:
        """停止 FeishuChannel 连接"""
        if self._channel is not None:
            try:
                await self._channel.disconnect()
                logger.info("[Lark] 飞书渠道已停止")
            except Exception:
                pass
            finally:
                self._channel = None
                self._handler = None

    async def health_check(self) -> bool:
        """检查飞书渠道是否正常运行"""
        return self._channel is not None


# ════════════════════════════════════════════════════════════════
# FastAPI lifespan 集成辅助函数
# ════════════════════════════════════════════════════════════════

_lark_manager: Optional[ChannelManager] = None


def get_lark_manager() -> Optional[ChannelManager]:
    """获取全局飞书渠道管理器"""
    return _lark_manager


async def startup_lark() -> Optional[ChannelManager]:
    """
    启动飞书渠道（在 FastAPI lifespan 启动阶段调用）。

    返回:
        ChannelManager 实例，如果凭据未配置或启动失败则返回 None
    """
    global _lark_manager

    config = FeishuConfig.from_env()
    if not config.app_id or not config.enabled:
        logger.info("[Lark] 飞书渠道未启用（凭据未配置或 FEISHU_ENABLED=false）")
        return None

    manager = ChannelManager(config)
    try:
        await manager.start()
    except Exception:
        logger.exception("[Lark] 飞书渠道启动失败")
        return None

    _lark_manager = manager
    return manager


async def shutdown_lark() -> None:
    """停止飞书渠道（在 FastAPI lifespan 关闭阶段调用）"""
    global _lark_manager

    if _lark_manager is not None:
        await _lark_manager.stop()
        _lark_manager = None
