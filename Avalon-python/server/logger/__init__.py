"""
Avalon 全局日志模块入口。

用法:
    from server.logger import logger, debug, info, warning, error, exception
"""

from .logger import debug, error, exception, info, logger, warning

__all__ = ["logger", "debug", "info", "warning", "error", "exception"]
