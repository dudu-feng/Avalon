"""
Memory 相关 Pydantic Schema
"""

from typing import List, Union

from pydantic import BaseModel, Field


class MemorySearchRequest(BaseModel):
    """记忆搜索请求"""

    query: str = Field(..., min_length=1, max_length=2000, description="搜索文本")
    search_mode: str = Field(
        default="hybrid",
        pattern="^(semantic|keyword|hybrid)$",
        description="检索模式: semantic / keyword / hybrid",
    )
    topk: int = Field(default=5, ge=1, le=20, description="返回结果数量")
    time_range: str = Field(default="", max_length=100, description="时间过滤")
