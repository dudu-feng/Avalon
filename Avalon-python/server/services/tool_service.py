"""
工具列表服务
"""

from tool import base_tool


def get_tools() -> list[dict]:
    """获取所有可用工具的元数据（名称、描述、参数）"""
    tools = []
    for tool in base_tool.TOOLS:
        tool_info = {
            "name": tool.name,
            "description": tool.description,
            "parameters": {},
        }
        # 尝试从 LangChain tool 的 args_schema 提取参数定义
        if hasattr(tool, "args_schema") and tool.args_schema:
            try:
                schema = tool.args_schema.model_json_schema()
                props = schema.get("properties", {})
                required = schema.get("required", [])
                for param_name, param_info in props.items():
                    tool_info["parameters"][param_name] = {
                        "type": param_info.get("type", "string"),
                        "required": param_name in required,
                        "description": param_info.get("description", ""),
                    }
            except Exception:
                pass
        tools.append(tool_info)
    return tools
