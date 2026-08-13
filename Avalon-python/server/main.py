"""
FastAPI 应用入口

生命周期:
  startup  → 初始化飞书渠道（如已配置凭据，建立 WebSocket 长连接）
  shutdown → 关闭飞书渠道连接
"""

import os
import sys
from contextlib import asynccontextmanager

# 将 Avalon-python 父目录加入 Python 路径，以便 `from server.xxx` 这类绝对导入生效
_project_dir = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
if _project_dir not in sys.path:
    sys.path.insert(0, _project_dir)

# 将 agent/ 目录加入 Python 路径，以便导入 loop / tool / config 等 core 模块
_agent_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "agent")
_agent_dir = os.path.abspath(_agent_dir)
if _agent_dir not in sys.path:
    sys.path.insert(0, _agent_dir)

from fastapi import FastAPI


def create_app() -> FastAPI:
    """创建并配置 FastAPI 应用（当前仅承载飞书渠道）"""
    app = FastAPI(
        title="Avalon API",
        version="1.0.0",
        description="Avalon 个人 AI 智能体（飞书渠道）",
    )

    # ── 飞书渠道 lifespan ──

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        """应用生命周期：启动/关闭飞书渠道"""
        from server.feishu_service import startup_lark, shutdown_lark

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
