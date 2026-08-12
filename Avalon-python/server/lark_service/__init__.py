"""
飞书/Lark 渠道 Adapter。

提供飞书机器人接入 Avalon 智能体的完整能力：
- 消息接收（WebSocket 长连接 / Webhook）
- 消息转换（InboundMessage → 标准 session entry）
- 回复输出（累积发送 / 流式输出）
- 卡片交互、系统事件、异常处理

顶层门面 LarkService 组装所有组件，暴露统一的 start()/stop()。
"""

import asyncio
from typing import Optional

from server.lark_service.config import FeishuConfig
from server.lark_service.channel import LarkChannel
from server.lark_service.converter import MessageConverter
from server.lark_service.adapter import ReplyAdapter
from server.lark_service.handler import EventHandler
from server.lark_service.pipeline import ReActPipeline

class LarkService:
    """
    飞书渠道顶层门面（Facade）。

    持有并协调所有子组件：
    - FeishuConfig: 配置（启动时从 .env 读取）
    - LarkChannel: SDK Channel 生命周期
    - EventHandler: 事件路由
    - ReActPipeline: ReAct 执行管线

    用法:
        service = LarkService()
        await service.start()
        # ... 应用运行中 ...
        await service.stop()
    """

    def __init__(self):
        self._config: Optional[FeishuConfig] = None
        self._channel: Optional[LarkChannel] = None
        self._handler: Optional[EventHandler] = None
        self._pipeline: Optional[ReActPipeline] = None

    async def start(self) -> None:
        """
        组装并启动飞书渠道。

        流程:
          ① 读取配置
          ② 创建 LarkChannel → 连接飞书 SDK
          ③ 创建 EventHandler + ReActPipeline
          ④ 绑定事件回调到 SDK Channel
        """
        self._config = FeishuConfig.from_env()
        if not self._config.is_configured():
            raise ValueError("飞书凭据未配置")

        # ① 创建管线
        self._pipeline = ReActPipeline()

        # ② 创建并启动 SDK Channel
        self._channel = LarkChannel(self._config)
        await self._channel.start()

        # ③ 创建事件处理器并绑定到 SDK Channel
        self._handler = EventHandler(self._config, self._pipeline)
        self._handler.bind(self._channel.sdk_channel)

    async def stop(self) -> None:
        """
        优雅停止飞书渠道。

        流程:
          ① 解绑事件回调（防止 disconnect 触发重连/错误回调）
          ② 断开 SDK Channel 连接
        """
        # ① 先解绑事件处理器
        if self._handler is not None and self._channel is not None:
            sdk = self._channel.sdk_channel
            if sdk is not None:
                self._handler.unbind(sdk)

        # ② 停止 SDK Channel
        if self._channel is not None:
            await self._channel.stop()

        self._handler = None
        self._channel = None
        self._pipeline = None
        self._config = None

# ════════════════════════════════════════════════════════════════
# 全局实例 + FastAPI lifespan 集成
# ════════════════════════════════════════════════════════════════

_lark_service: Optional[LarkService] = None


def get_lark_service() -> Optional[LarkService]:
    """获取全局飞书服务实例"""
    return _lark_service


async def startup_lark() -> Optional[LarkService]:
    """
    启动飞书渠道（在 FastAPI lifespan 启动阶段调用）。

    返回:
        LarkService 实例，如果凭据未配置或启动失败则返回 None
    """
    global _lark_service

    config = FeishuConfig.from_env()
    if not config.app_id or not config.enabled:
        return None

    service = LarkService()
    try:
        await service.start()
    except Exception:
        return None

    _lark_service = service
    return service


async def shutdown_lark() -> None:
    """停止飞书渠道（在 FastAPI lifespan 关闭阶段调用）"""
    global _lark_service

    if _lark_service is not None:
        try:
            await _lark_service.stop()
        except asyncio.CancelledError:
            # FastAPI lifespan 被 KeyboardInterrupt 取消时，
            # uvicorn 会抛出 CancelledError，静默处理即可。
            pass
        _lark_service = None


# ════════════════════════════════════════════════════════════════
# 公开导出
# ════════════════════════════════════════════════════════════════

__all__ = [
    "FeishuConfig",
    "LarkChannel",
    "MessageConverter",
    "ReplyAdapter",
    "EventHandler",
    "ReActPipeline",
    "LarkService",
    "startup_lark",
    "shutdown_lark",
    "get_lark_service",
]
