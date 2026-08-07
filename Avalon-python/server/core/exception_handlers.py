"""
全局异常处理器注册
"""

import traceback

from fastapi import FastAPI, Request
from fastapi.exceptions import RequestValidationError
from fastapi.responses import JSONResponse

from server.core.exceptions import AvalonException
from server.schemas.common import ApiResponse


def register_handlers(app: FastAPI) -> None:
    """向 FastAPI 应用注册全部异常处理器"""

    @app.exception_handler(AvalonException)
    async def avalon_exception_handler(
        request: Request, exc: AvalonException
    ) -> JSONResponse:
        return JSONResponse(
            status_code=exc.http_status,
            content=ApiResponse.fail(exc.code, exc.message).model_dump(),
        )

    @app.exception_handler(RequestValidationError)
    async def validation_handler(
        request: Request, exc: RequestValidationError
    ) -> JSONResponse:
        detail = exc.errors()[0]["msg"] if exc.errors() else "参数校验失败"
        return JSONResponse(
            status_code=400,
            content=ApiResponse.fail(40004, detail).model_dump(),
        )

    @app.exception_handler(Exception)
    async def general_exception_handler(
        request: Request, exc: Exception
    ) -> JSONResponse:
        traceback.print_exc()
        return JSONResponse(
            status_code=500,
            content=ApiResponse.fail(50000, "服务器内部错误").model_dump(),
        )
