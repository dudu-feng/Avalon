"""
配置管理服务
"""

import os
import re

from config.env_config import env_config
from server.core.exceptions import ConfigWriteException


def _mask_key(key: str) -> str:
    """API Key 脱敏：保留前3位 + **** + 后4位"""
    if not key or len(key) <= 7:
        return "****"
    return key[:3] + "****" + key[-4:]


def get_config() -> dict:
    """返回当前配置（API Key 脱敏）"""
    return {
        "model": {
            "name": env_config.default_model,
            "base_url": env_config.default_model_base_url,
            "api_key": _mask_key(env_config.default_api_key),
        },
        "session": {
            "compress_threshold": env_config.session_memory_compress_threshold,
            "max_chunks": env_config.session_memory_max_chunks,
            "context_chunks": env_config.session_memory_context_chunks,
        },
        "embedding": {
            "mode": env_config.embedding_mode,
            "model_name": env_config.local_embedding_model,
            "device": env_config.embedding_device,
        },
    }


def _find_env_path() -> str:
    """定位 .env 文件路径（agent/.env）"""
    import inspect
    # 通过 inspect 获取 env_config 类所在文件的真实路径
    module_file = inspect.getfile(env_config.__class__)
    config_dir = os.path.dirname(module_file)   # agent/config/
    agent_dir = os.path.dirname(config_dir)     # agent/
    return os.path.join(agent_dir, ".env")


# .env key → 配置项 path 映射
_KEY_MAP = {
    "default_api_key": "model.api_key",
    "default_model": "model.name",
    "default_model_base_url": "model.base_url",
    "session_memory_compress_threshold": "session.compress_threshold",
    "session_memory_max_chunks": "session.max_chunks",
    "session_memory_context_chunks": "session.context_chunks",
    "embedding_mode": "embedding.mode",
    "local_embedding_model": "embedding.model_name",
    "embedding_device": "embedding.device",
}

# 需要重启才能生效的字段
_NEEDS_RESTART_FIELDS = {
    "embedding.mode",
    "embedding.model_name",
    "embedding.device",
}


def update_config(updates: dict) -> dict:
    """
    写回 .env 文件。

    Args:
        updates: {"model": {...}, "session": {...}, "embedding": {...}}

    Returns:
        {"updated": [...], "needs_restart": bool}
    """
    # 将嵌套结构拍平为 .env key → value
    flat_updates = {}

    if updates.get("model"):
        m = updates["model"]
        if m.get("name") is not None:
            flat_updates["default_model"] = m["name"]
        if m.get("base_url") is not None:
            flat_updates["default_model_base_url"] = m["base_url"]
        if m.get("api_key") is not None:
            flat_updates["default_api_key"] = m["api_key"]

    if updates.get("session"):
        s = updates["session"]
        if s.get("compress_threshold") is not None:
            flat_updates["session_memory_compress_threshold"] = str(s["compress_threshold"])
        if s.get("max_chunks") is not None:
            flat_updates["session_memory_max_chunks"] = str(s["max_chunks"])
        if s.get("context_chunks") is not None:
            flat_updates["session_memory_context_chunks"] = str(s["context_chunks"])

    if updates.get("embedding"):
        e = updates["embedding"]
        if e.get("mode") is not None:
            flat_updates["embedding_mode"] = e["mode"]
        if e.get("model_name") is not None:
            flat_updates["local_embedding_model"] = e["model_name"]
        if e.get("device") is not None:
            flat_updates["embedding_device"] = e["device"]

    if not flat_updates:
        return {"updated": [], "needs_restart": False}

    # 写回 .env 文件
    env_path = _find_env_path()
    if not os.path.isfile(env_path):
        raise ConfigWriteException(f".env 文件不存在: {env_path}")

    try:
        with open(env_path, "r", encoding="utf-8") as f:
            lines = f.readlines()
    except Exception as e:
        raise ConfigWriteException(f"读取 .env 文件失败: {e}")

    updated_paths = []
    for env_key, value in flat_updates.items():
        replaced = False
        for i, line in enumerate(lines):
            if re.match(rf"^{env_key}\s*=", line.strip()):
                lines[i] = f"{env_key}={value}\n"
                replaced = True
                break
        if not replaced:
            lines.append(f"{env_key}={value}\n")

        config_path = _KEY_MAP.get(env_key, env_key)
        updated_paths.append(config_path)

    try:
        with open(env_path, "w", encoding="utf-8") as f:
            f.writelines(lines)
    except Exception as e:
        raise ConfigWriteException(f"写入 .env 文件失败: {e}")

    # 热更新加载
    try:
        env_config._load_dotenv()
    except Exception:
        pass

    needs_restart = any(p in _NEEDS_RESTART_FIELDS for p in updated_paths)

    return {"updated": updated_paths, "needs_restart": needs_restart}
