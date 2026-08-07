"""
记忆检索服务
"""

import json

from tool.session_memory_tool import search_session_memory
from server.core.exceptions import VectorDBException


def search_memory(
    query: str,
    search_mode: str = "hybrid",
    topk: int = 5,
    time_range: str = "",
) -> dict:
    """
    搜索历史会话记忆。

    直接复用 session_memory_tool 的 search_session_memory，
    将返回的 JSON 字符串解析为 dict。
    """
    try:
        result_str = search_session_memory.invoke({
            "query": query,
            "search_mode": search_mode,
            "topk": topk,
            "time_range": time_range,
        })
        result = json.loads(result_str)
    except Exception as e:
        raise VectorDBException(f"记忆检索失败: {e}")

    # 如果是错误响应
    if isinstance(result, dict) and "error" in result:
        raise VectorDBException(result["error"])

    # 如果是 {"message": "未找到..."}，标准化为 results 数组
    if isinstance(result, dict) and "message" in result:
        return {"results": result.get("results", []), "total": 0}

    # 直接是数组格式
    if isinstance(result, list):
        return {"results": result, "total": len(result)}

    return {"results": [], "total": 0}
