"""
Avalon 全局日志模块。

print() 在 uvicorn 服务里不可靠（stdout 可能被缓冲/重定向，且缺少时间戳与来源），
这里统一收敛到 logging：输出到 stdout 并实时 flush，任意位置 import 即用。

用法:
    from server.logger import logger
    logger.debug("收到消息: %s", text)

    # 或使用模块级便捷函数（用法接近 print）
    from server.logger import debug, info, warning, error, exception
    info("收到消息: %s", text)
"""

import logging
import sys

_NAME = "avalon"
_FORMAT = "%(asctime)s | %(levelname)-7s | %(filename)s:%(lineno)d | %(message)s"
_DATE_FORMAT = "%Y-%m-%d %H:%M:%S"


def _build_logger() -> logging.Logger:
    """构建全局 logger 单例，重复导入不会重复添加 handler。"""
    lg = logging.getLogger(_NAME)
    if lg.handlers:
        return lg

    lg.setLevel(logging.DEBUG)
    # 关闭向上传播，避免被 uvicorn / root logger 的配置拦截
    lg.propagate = False

    handler = logging.StreamHandler(sys.stdout)
    handler.setFormatter(logging.Formatter(_FORMAT, _DATE_FORMAT))
    lg.addHandler(handler)
    return lg


logger = _build_logger()


def debug(msg: object, *args, **kwargs) -> None:
    """调试日志：打印变量值、中间状态等。"""
    kwargs.setdefault("stacklevel", 2)
    logger.debug(msg, *args, **kwargs)


def info(msg: object, *args, **kwargs) -> None:
    """常规信息日志。"""
    kwargs.setdefault("stacklevel", 2)
    logger.info(msg, *args, **kwargs)


def warning(msg: object, *args, **kwargs) -> None:
    """警告日志。"""
    kwargs.setdefault("stacklevel", 2)
    logger.warning(msg, *args, **kwargs)


def error(msg: object, *args, **kwargs) -> None:
    """错误日志。"""
    kwargs.setdefault("stacklevel", 2)
    logger.error(msg, *args, **kwargs)


def exception(msg: object, *args, **kwargs) -> None:
    """异常日志：在 except 块内使用，自动附带堆栈。"""
    kwargs.setdefault("stacklevel", 2)
    logger.exception(msg, *args, **kwargs)


__all__ = ["logger", "debug", "info", "warning", "error", "exception"]
