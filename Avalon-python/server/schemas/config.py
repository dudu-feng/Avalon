"""
Config 相关 Pydantic Schema
"""

from typing import List, Optional

from pydantic import BaseModel


class ModelConfigPart(BaseModel):
    name: Optional[str] = None
    base_url: Optional[str] = None
    api_key: Optional[str] = None


class SessionConfigPart(BaseModel):
    compress_threshold: Optional[int] = None
    max_chunks: Optional[int] = None
    context_chunks: Optional[int] = None


class EmbeddingConfigPart(BaseModel):
    mode: Optional[str] = None
    model_name: Optional[str] = None
    device: Optional[str] = None


class ConfigUpdateRequest(BaseModel):
    model: Optional[ModelConfigPart] = None
    session: Optional[SessionConfigPart] = None
    embedding: Optional[EmbeddingConfigPart] = None


class ConfigUpdateResponse(BaseModel):
    updated: List[str] = []
    needs_restart: bool = False
