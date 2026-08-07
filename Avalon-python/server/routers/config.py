"""
Config API 路由
"""

from fastapi import APIRouter

from server.schemas.common import ApiResponse
from server.schemas.config import ConfigUpdateRequest, ConfigUpdateResponse
from server.services.config_service import get_config, update_config

router = APIRouter(prefix="/config", tags=["Config"])


@router.get("", response_model=ApiResponse[dict])
async def api_get_config():
    """获取当前配置（API Key 脱敏）"""
    data = get_config()
    return ApiResponse.ok(data)


@router.put("", response_model=ApiResponse[ConfigUpdateResponse])
async def api_update_config(req: ConfigUpdateRequest):
    """更新配置项"""
    data = update_config(req.model_dump(exclude_none=True))
    return ApiResponse.ok(data)
