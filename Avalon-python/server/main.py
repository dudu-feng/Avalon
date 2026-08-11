"""
FastAPI 应用入口

生命周期:
  startup  → 初始化飞书渠道（如已配置凭据）
  shutdown → 关闭飞书渠道连接
"""

import os
import sys
from contextlib import asynccontextmanager

# 将 agent/ 目录加入 Python 路径，以便导入 loop / tool / config 等 core 模块
_agent_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "agent")
_agent_dir = os.path.abspath(_agent_dir)
if _agent_dir not in sys.path:
    sys.path.insert(0, _agent_dir)

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from server.core.exception_handlers import register_handlers


def create_app() -> FastAPI:
    """创建并配置 FastAPI 应用"""
    app = FastAPI(
        title="Avalon API",
        version="1.0.0",
        description="Avalon 个人 AI 智能体 REST API + SSE 流式接口",
    )

    # CORS — 开发阶段开放所有来源
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )

    # 注册路由
    from server.routers.chat import router as chat_router
    from server.routers.session import router as session_router
    from server.routers.memory import router as memory_router
    from server.routers.config import router as config_router
    from server.routers.tool import router as tool_router

    app.include_router(chat_router, prefix="/api/v1")
    app.include_router(session_router, prefix="/api/v1")
    app.include_router(memory_router, prefix="/api/v1")
    app.include_router(config_router, prefix="/api/v1")
    app.include_router(tool_router, prefix="/api/v1")

    # 注册异常处理器
    register_handlers(app)

    # ── 飞书渠道 lifespan ──

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        """应用生命周期：启动/关闭飞书渠道"""
        from server.lark_service.channel_manager import startup_lark, shutdown_lark

        await startup_lark()
        try:
            yield
        finally:
            await shutdown_lark()

    app.router.lifespan_context = lifespan

    return app


# 模块级 app 实例，供 uvicorn 使用
app = create_app()


def main():
    """开发环境启动入口"""
    import uvicorn

    uvicorn.run(
        "server.main:app",
        host="0.0.0.0",
        port=8000,
        reload=False,
    )


if __name__ == "__main__":
    main()
