"""
Chat 相关 Pydantic Schema
"""

from pydantic import BaseModel, Field


class SendMessageRequest(BaseModel):
    """发送消息请求"""

    session_id: str = Field(..., min_length=1, max_length=200, description="目标会话 ID")
    message: str = Field(..., min_length=1, max_length=100000, description="用户输入文本")
