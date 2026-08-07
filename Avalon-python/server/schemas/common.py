"""
统一 API 响应模型
"""

from typing import Generic, Optional, TypeVar

from pydantic import BaseModel

T = TypeVar("T")


class ApiResponse(BaseModel, Generic[T]):
    """所有非流式接口的统一响应包装"""

    code: int = 0
    message: str = "success"
    data: Optional[T] = None

    @classmethod
    def ok(cls, data: T = None, message: str = "success") -> "ApiResponse[T]":
        return cls(code=0, message=message, data=data)

    @classmethod
    def fail(cls, code: int, message: str) -> "ApiResponse[None]":
        return cls(code=code, message=message, data=None)
