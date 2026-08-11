"""
会话管理 API 路由
"""

from fastapi import APIRouter, Query

from server.schemas.common import ApiResponse
from server.schemas.session import (
    ArchiveResponse,
    CompressResponse,
    CreateSessionRequest,
    CreateSessionResponse,
    DeleteResponse,
    SessionListData,
)
from server.services.session_service import (
    archive_session,
    compress_session,
    create_session,
    delete_session,
    get_raw_chunk,
    get_session,
    list_sessions,
)

router = APIRouter(prefix="/sessions", tags=["Session"])


@router.get("", response_model=ApiResponse[SessionListData])
async def api_list_sessions():
    """获取所有会话列表（当前活跃 + 历史归档）"""
    data = list_sessions()
    return ApiResponse.ok(data)


@router.post("", response_model=ApiResponse[CreateSessionResponse])
async def api_create_session(req: CreateSessionRequest):
    """创建新会话"""
    data = create_session(req.channel, req.preserve_current)
    return ApiResponse.ok(data)


@router.get("/{session_id}", response_model=ApiResponse[dict])
async def api_get_session(session_id: str):
    """获取会话完整详情"""
    data = get_session(session_id)
    return ApiResponse.ok(data)


@router.get("/{session_id}/raw/{chunk}", response_model=ApiResponse[dict])
async def api_get_raw_chunk(session_id: str, chunk: str):
    """获取压缩块的原始对话内容"""
    data = get_raw_chunk(session_id, chunk)
    return ApiResponse.ok(data)


@router.post("/{session_id}/compress", response_model=ApiResponse[CompressResponse])
async def api_compress_session(session_id: str):
    """手动触发会话压缩"""
    data = compress_session(session_id)
    return ApiResponse.ok(data)


@router.post("/{session_id}/archive", response_model=ApiResponse[ArchiveResponse])
async def api_archive_session(session_id: str):
    """归档活跃会话"""
    data = archive_session(session_id)
    return ApiResponse.ok(data)


@router.delete("/{session_id}", response_model=ApiResponse[DeleteResponse])
async def api_delete_session(session_id: str):
    """删除历史会话（含文件和向量数据）"""
    data = delete_session(session_id)
    return ApiResponse.ok(data)
