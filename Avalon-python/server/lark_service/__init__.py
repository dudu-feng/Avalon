"""
飞书/Lark 渠道 Adapter。

提供飞书机器人接入 Avalon 智能体的完整能力：
- 消息接收（WebSocket 长连接 / Webhook）
- 消息转换（InboundMessage → 标准 session entry）
- 回复输出（累积发送 / 流式输出）
- 卡片交互、系统事件、异常处理
"""

from server.lark_service.config import FeishuConfig
from server.lark_service.channel_manager import ChannelManager
from server.lark_service.event_handler import EventHandler

__all__ = ["FeishuConfig", "ChannelManager", "EventHandler"]
