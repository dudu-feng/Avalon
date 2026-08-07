"""
业务异常类层次
"""


class AvalonException(Exception):
    """业务异常基类"""

    code: int = 50000
    message: str = "服务器内部错误"
    detail: str = ""
    http_status: int = 500

    def __init__(self, detail: str = "", message: str = ""):
        self.detail = detail or self.message
        if message:
            self.message = message
        super().__init__(self.message)


# ============================================================
# 4xxx — 客户端错误
# ============================================================


class SessionEmptyForCompressException(AvalonException):
    code = 40001
    message = "当前会话无消息可压缩"
    http_status = 400


class ActiveSessionDeleteForbiddenException(AvalonException):
    code = 40002
    message = "活跃会话不可删除，请先归档"
    http_status = 400


class SessionNotFoundException(AvalonException):
    code = 40003
    message = "会话不存在"
    http_status = 404


class ValidationException(AvalonException):
    code = 40004
    message = "请求参数校验失败"
    http_status = 400


# ============================================================
# 5xxx — 服务端错误
# ============================================================


class LLMCallException(AvalonException):
    code = 50001
    message = "LLM 调用失败或超时"
    http_status = 500


class LLMParseException(AvalonException):
    code = 50002
    message = "LLM 返回的 JSON 无法解析"
    http_status = 500


class ToolExecutionException(AvalonException):
    code = 50003
    message = "工具执行异常"
    http_status = 500


class CompressException(AvalonException):
    code = 50004
    message = "压缩模型调用失败"
    http_status = 500


class VectorDBException(AvalonException):
    code = 50005
    message = "向量数据库操作失败"
    http_status = 500


class FileIOException(AvalonException):
    code = 50006
    message = "文件读写失败"
    http_status = 500


class ConfigWriteException(AvalonException):
    code = 50007
    message = "配置写入失败"
    http_status = 500
