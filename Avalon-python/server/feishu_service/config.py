"""
飞书渠道配置。

从 .env 读取飞书开放平台凭据与开关。
"""

import os
from dataclasses import dataclass

# 触发 .env 加载（项目约定：env_config 是唯一的环境变量加载入口）
from config.env_config import env_config  # noqa: F401


@dataclass
class FeishuConfig:
    app_id: str = ""
    app_secret: str = ""
    enabled: bool = False
    transport: str = "ws"                # ws / webhook
    domain: str = ""                     # 可选，飞书自定义域名
    encrypt_key: str = ""
    verification_token: str = ""
    log_level: str = "INFO"
    dm_enabled: bool = True              # 私聊开关
    group_enabled: bool = True           # 群聊开关
    require_mention: bool = False        # 群聊是否要求 @机器人
    respond_to_mention_all: bool = False # 是否响应 @所有人
    dedup_ttl_hours: int = 2             # 消息去重窗口（小时）
    processing_reaction: str = ""        # 处理中表情
    done_reaction: str = ""              # 完成表情

    @classmethod
    def from_env(cls) -> "FeishuConfig":
        def _bool(key: str, default: bool) -> bool:
            return os.getenv(key, str(default)).strip().lower() in ("true", "1", "yes", "on")

        def _int(key: str, default: int) -> int:
            try:
                return int(os.getenv(key, str(default)))
            except (TypeError, ValueError):
                return default

        return cls(
            app_id=os.getenv("LARK_APP_ID", ""),
            app_secret=os.getenv("LARK_APP_SECRET", ""),
            enabled=_bool("FEISHU_ENABLED", False),
            transport=os.getenv("LARK_TRANSPORT", "ws"),
            domain=os.getenv("LARK_DOMAIN", ""),
            encrypt_key=os.getenv("LARK_ENCRYPT_KEY", ""),
            verification_token=os.getenv("LARK_VERIFICATION_TOKEN", ""),
            log_level=os.getenv("FEISHU_LOG_LEVEL", "INFO"),
            dm_enabled=_bool("FEISHU_DM_ENABLED", True),
            group_enabled=_bool("FEISHU_GROUP_ENABLED", True),
            require_mention=_bool("FEISHU_REQUIRE_MENTION", False),
            respond_to_mention_all=_bool("FEISHU_RESPOND_TO_MENTION_ALL", False),
            dedup_ttl_hours=_int("FEISHU_DEDUP_TTL_HOURS", 2),
            processing_reaction=os.getenv("FEISHU_PROCESSING_REACTION", ""),
            done_reaction=os.getenv("FEISHU_DONE_REACTION", ""),
        )

    def is_configured(self) -> bool:
        """是否配置了必要的飞书凭据"""
        return bool(self.app_id and self.app_secret)
