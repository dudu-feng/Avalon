"""ReAct 事件发射器。

集中定义 react_loop 对外推送的全部事件契约。
每个方法对应一类事件，方法签名即事件的数据字段，
避免在核心循环里散落事件名字符串和 dict 构造。

事件契约总览：

| 事件名              | data 字段                  | 说明                     |
|---------------------|----------------------------|--------------------------|
| chat_message        | delta                      | 回复正文（含降级纯文本） |
| action_start        | action_target              | 进入动作层               |
| action_step         | analysis, next             | 动作层每步分析           |
| action_tool_call    | tool_name, arguments       | 调用工具                 |
| action_tool_result  | tool_name, success, result | 工具结果                 |
| action_sub_analysis | analysis, sub_analysis     | 子分析                   |
| action_finished     | analysis, token_usage      | 动作完成                 |
| error               | code, message              | 异常                     |
"""

from typing import Callable


class ReactEmitter:
    """把 on_event 回调封装成语义化的事件发射方法。"""

    def __init__(self, on_event: Callable[[str, dict], None] | None = None):
        self._on_event = on_event

    def _emit(self, event_type: str, data: dict) -> None:
        if self._on_event:
            self._on_event(event_type, data)

    def chat_message(self, delta: str) -> None:
        self._emit("chat_message", {"delta": delta})

    def action_start(self, action_target: str) -> None:
        self._emit("action_start", {"action_target": action_target})

    def action_step(self, analysis: str, next_step: str) -> None:
        self._emit("action_step", {"analysis": analysis, "next": next_step})

    def action_tool_call(self, tool_name: str, arguments: dict) -> None:
        self._emit("action_tool_call", {"tool_name": tool_name, "arguments": arguments})

    def action_tool_result(self, tool_name: str, success: bool, result: str) -> None:
        self._emit("action_tool_result", {"tool_name": tool_name, "success": success, "result": result})

    def action_sub_analysis(self, analysis: str, sub_analysis: str) -> None:
        self._emit("action_sub_analysis", {"analysis": analysis, "sub_analysis": sub_analysis})

    def action_finished(self, analysis: str, token_usage: dict) -> None:
        self._emit("action_finished", {"analysis": analysis, "token_usage": token_usage})

    def error(self, code: int, message: str) -> None:
        self._emit("error", {"code": code, "message": message})
