"""
飞书渠道服务层。

对外暴露：
  - 配置（FeishuConfig）
  - SDK 服务（FeishuSDKService / create_sdk / destroy_sdk）
  - 基础适配层（handle_feishu_message）
  - 生命周期（startup_lark / shutdown_lark，供 FastAPI lifespan 调用）
"""

from server.feishu_service.config import FeishuConfig
from server.feishu_service.feishu_sdk import (
    FeishuEvent,
    create_sdk,
    destroy_sdk,
    get_sdk,
)
from server.feishu_service.adapter import handle_feishu_message


async def startup_lark() -> None:
    """启动飞书渠道：凭据已配置时连接 SDK 并注册消息处理器"""
    config = FeishuConfig.from_env()
    if not config.enabled or not config.is_configured():
        return

    # 会话初始化只需一次：确保 session 文件存在，后续每条消息复用同一会话
    from loop import session_manage
    session_manage.init_session("feishu")

    sdk = await create_sdk(config)
    sdk.on(FeishuEvent.MESSAGE, handle_feishu_message)


async def shutdown_lark() -> None:
    """关闭飞书渠道"""
    await destroy_sdk()


__all__ = [
    "FeishuConfig",
    "FeishuEvent",
    "get_sdk",
    "create_sdk",
    "destroy_sdk",
    "handle_feishu_message",
    "startup_lark",
    "shutdown_lark",
]
