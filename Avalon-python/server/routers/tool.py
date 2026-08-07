"""
Tool API 路由
"""

from fastapi import APIRouter

from server.schemas.common import ApiResponse
from server.services.tool_service import get_tools

router = APIRouter(prefix="/tools", tags=["Tool"])


@router.get("", response_model=ApiResponse[dict])
async def api_get_tools():
    """获取可用工具列表"""
    data = {"tools": get_tools()}
    return ApiResponse.ok(data)
