"""
SSE (Server-Sent Events) 工具函数
"""

import json


def format_sse(event_id: int, event_type: str, data: dict) -> str:
    """将事件格式化为标准 SSE 字符串

    Args:
        event_id: 事件序号（递增整数）
        event_type: 事件类型（如 chat_start, chat_message, done 等）
        data: 事件负载（dict，自动序列化为 JSON）

    Returns:
        符合 SSE 规范的字符串（含换行）
    """
    lines = [
        f"id: {event_id}",
        f"event: {event_type}",
        f"data: {json.dumps(data, ensure_ascii=False)}",
        "",  # 空行分隔
    ]
    return "\n".join(lines) + "\n"
