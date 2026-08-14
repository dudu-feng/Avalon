"""
飞书媒体转写层。

职责：把飞书消息里的媒体资源（图片/语音）转成文字，供纯文本 LLM（DeepSeek）理解。

当前实现：
  - transcribe_audio：下载语音 → faster-whisper 本地转写
  - describe_image：下载图片 → 视觉大模型描述（待 VISION key 就绪后启用）
"""

import asyncio
import os
import tempfile
from typing import Optional

from server.feishu_service.feishu_sdk import get_sdk
from server.logger import logger


# faster-whisper 懒加载单例：首次调用时才加载模型，避免启动即占用内存
_whisper_model = None


def _get_whisper_model():
    """懒加载 faster-whisper 模型（单例），失败返回 None。"""
    global _whisper_model
    if _whisper_model is not None:
        return _whisper_model

    from config.env_config import env_config

    try:
        from faster_whisper import WhisperModel
    except ImportError:
        logger.warning("未安装 faster-whisper，语音转写不可用")
        return None

    model_name = env_config.whisper_model_name or "medium"
    model_path = env_config.whisper_model_path or None
    device = env_config.whisper_device or "cpu"
    compute_type = "int8" if device == "cpu" else "float16"

    try:
        kwargs: dict = {"device": device, "compute_type": compute_type}
        if model_path:
            kwargs["download_root"] = model_path
        _whisper_model = WhisperModel(model_name, **kwargs)
    except Exception:
        logger.exception("加载 Whisper 模型失败")
        return None
    return _whisper_model


def is_whisper_loaded() -> bool:
    """Whisper 模型是否已加载（True = 热，转写快；False = 冷启动，转写慢）"""
    return _whisper_model is not None


def _transcribe_bytes(data: bytes) -> str:
    """同步：落临时文件 → 转写 → 清理，返回文字（失败返回空串）。"""
    model = _get_whisper_model()
    if model is None:
        return ""

    from config.env_config import env_config

    # 临时文件写到 file_path/temp 目录（由 .env 的 file_path 控制），
    # 未配置时回退到系统默认临时目录
    temp_dir = env_config.temp_file_path
    if temp_dir:
        os.makedirs(temp_dir, exist_ok=True)

    tmp_path = None
    try:
        with tempfile.NamedTemporaryFile(
            suffix=".ogg", delete=False, dir=temp_dir or None
        ) as f:
            f.write(data)
            tmp_path = f.name

        # language="zh" 针对中文语音；如需中英混用可改为 language=None 自动检测
        segments, _info = model.transcribe(tmp_path, language="zh")
        return "".join(seg.text for seg in segments).strip()
    except Exception:
        logger.exception("语音转写失败")
        return ""
    finally:
        if tmp_path:
            try:
                os.remove(tmp_path)
            except OSError:
                pass


async def transcribe_audio(file_key: str, message_id: Optional[str] = None) -> str:
    """下载语音并转写为文字，失败返回空字符串。"""
    sdk = get_sdk()
    if sdk is None:
        return ""

    try:
        # 飞书下载语音资源时 type 参数须为 "file"：GetMessageResourceRequest
        # 的 type 仅接受 image/file，语音的 file_key 走 message-resource 端点
        # 用 type=file 下载（传 "audio" 会得到 200 但无文件内容）。
        data = await sdk.download_resource(file_key, "file", message_id)
    except Exception:
        logger.warning("语音下载失败: file_key=%s", file_key)
        return ""
    if not data:
        return ""

    # 转写是 CPU 密集操作，放到线程池避免阻塞事件循环
    loop = asyncio.get_event_loop()
    return await loop.run_in_executor(None, _transcribe_bytes, data)
