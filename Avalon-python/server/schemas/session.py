"""
会话相关 Pydantic Schema
"""

from typing import List, Optional

from pydantic import BaseModel, Field


# ============================================================
# 请求体
# ============================================================


class CreateSessionRequest(BaseModel):
    channel: str = "web"
    preserve_current: bool = True


# ============================================================
# 响应体
# ============================================================


class SessionMeta(BaseModel):
    """会话摘要（列表用，不含完整消息）"""

    id: str
    status: str
    compress_round: int
    message_count: int
    last_message_time: Optional[str] = None


class SessionListData(BaseModel):
    current: List[SessionMeta] = []
    history: List[SessionMeta] = []


class CreateSessionResponse(BaseModel):
    id: str
    status: str
    created_at: str


class CompressResponse(BaseModel):
    session_id: str
    compress_round: int
    archived_messages: int = 0
    chunk_summary: List[str] = []
    chunk_keywords: List[str] = []
    progressive_merged: bool = False


class ArchiveResponse(BaseModel):
    session_id: str
    archived_at: str


class DeleteResponse(BaseModel):
    session_id: str
    deleted_files: int
    zvec_entries_removed: int
