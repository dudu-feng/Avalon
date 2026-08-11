"""
飞书渠道配置。

从 .env 读取飞书应用凭据和策略参数，所有配置项都有合理的默认值。
"""

import os
from dataclasses import dataclass, field


@dataclass
class FeishuConfig:
    """飞书渠道完整配置"""

    # ── 凭据（必填） ──
    app_id: str = ""
    app_secret: str = ""

    # ── 开关 ──
    enabled: bool = True

    # ── 连接 ──
    domain: str = "https://open.feishu.cn"
    transport: str = "ws"              # "ws" | "webhook"

    # ── Webhook 模式专属 ──
    encrypt_key: str = ""
    verification_token: str = ""

    # ── 策略 ──
    dm_enabled: bool = True            # 是否接收单聊消息
    group_enabled: bool = True         # 是否接收群聊消息
    require_mention: bool = True       # 群聊是否要求显式 @bot
    respond_to_mention_all: bool = False  # 是否响应 @all
    dedup_ttl_hours: int = 12          # 消息去重窗口

    # ── Reaction 表情标记 ──
    processing_reaction: str = "👀"     # 处理中（打在用户消息上）
    done_reaction: str = "✅"           # 处理完成

    # ── 日志 ──
    log_level: str = "INFO"

    def is_configured(self) -> bool:
        """凭据是否已配置"""
        return bool(self.app_id and self.app_secret)

    @classmethod
    def from_env(cls) -> "FeishuConfig":
        """
        从环境变量读取配置。

        变量名                默认值
        ─────────────────────────────────────────
        LARK_APP_ID           (空)
        LARK_APP_SECRET       (空)
        FEISHU_ENABLED        true
        FEISHU_DOMAIN         open.feishu.cn
        FEISHU_TRANSPORT      ws
        LARK_ENCRYPT_KEY      (空)
        LARK_VERIFICATION_TOKEN  (空)
        FEISHU_DM_ENABLED     true
        FEISHU_GROUP_ENABLED  true
        FEISHU_REQUIRE_MENTION  true
        FEISHU_DEDUP_TTL_HOURS  12
        FEISHU_PROCESSING_REACTION  👀
        FEISHU_DONE_REACTION      ✅
        """
        return cls(
            app_id=os.getenv("LARK_APP_ID", ""),
            app_secret=os.getenv("LARK_APP_SECRET", ""),
            enabled=os.getenv("FEISHU_ENABLED", "true").strip().lower() == "true",
            domain=os.getenv("FEISHU_DOMAIN", "https://open.feishu.cn"),
            transport=os.getenv("FEISHU_TRANSPORT", "ws").strip().lower(),
            encrypt_key=os.getenv("LARK_ENCRYPT_KEY", ""),
            verification_token=os.getenv("LARK_VERIFICATION_TOKEN", ""),
            dm_enabled=os.getenv("FEISHU_DM_ENABLED", "true").strip().lower() == "true",
            group_enabled=os.getenv("FEISHU_GROUP_ENABLED", "true").strip().lower() == "true",
            require_mention=os.getenv("FEISHU_REQUIRE_MENTION", "true").strip().lower() == "true",
            respond_to_mention_all=os.getenv("FEISHU_RESPOND_MENTION_ALL", "false").strip().lower() == "true",
            dedup_ttl_hours=int(os.getenv("FEISHU_DEDUP_TTL_HOURS", "12")),
            processing_reaction=os.getenv("FEISHU_PROCESSING_REACTION", "👀"),
            done_reaction=os.getenv("FEISHU_DONE_REACTION", "✅"),
            log_level=os.getenv("FEISHU_LOG_LEVEL", "INFO").strip().upper(),
        )
