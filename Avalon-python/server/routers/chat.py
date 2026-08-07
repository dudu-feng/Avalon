"""
Chat API 路由
"""

from fastapi import APIRouter, Request
from fastapi.responses import StreamingResponse

from server.schemas.chat import SendMessageRequest
from server.services.chat_service import generate_sse

router = APIRouter(prefix="/chat", tags=["Chat"])


@router.post("/send")
async def send_message(req: SendMessageRequest):
    """
    发送消息，SSE 流式返回 ReAct 双层循环全过程。

    事件类型（按顺序）:
      chat_start → chat_thought → chat_message → chat_stop → done
      或:
      chat_start → ... → action_start → action_step → action_tool_call
                → action_tool_result → action_finished → chat_message → done
    """
    return StreamingResponse(
        generate_sse(req.session_id, req.message),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "X-Accel-Buffering": "no",  # 禁用 Nginx 缓冲
        },
    )
