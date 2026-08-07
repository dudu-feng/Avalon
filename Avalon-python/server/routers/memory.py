"""
Memory API 路由
"""

from fastapi import APIRouter

from server.schemas.common import ApiResponse
from server.schemas.memory import MemorySearchRequest
from server.services.memory_service import search_memory

router = APIRouter(tags=["Memory"])


@router.post("/memory/search", response_model=ApiResponse[dict])
async def api_search_memory(req: MemorySearchRequest):
    """搜索历史会话记忆"""
    data = search_memory(
        query=req.query,
        search_mode=req.search_mode,
        topk=req.topk,
        time_range=req.time_range,
    )
    return ApiResponse.ok(data)
