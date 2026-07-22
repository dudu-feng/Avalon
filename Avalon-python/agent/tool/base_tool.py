import os
import subprocess
from langchain_core.tools import tool

@tool
def read_file(file_path: str) -> str:
    """读取指定文件的内容，传入参数 file_path 为文件路径"""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            return f.read()
    except Exception as e:
        return f"读取文件失败: {e}"

@tool
def write_file(file_path: str, content: str) -> str:
    """创建或覆盖写入文件，传入 file_path 和 content"""
    try:
        with open(file_path, 'w', encoding='utf-8') as f:
            f.write(content)
        return f"文件 {file_path} 写入成功"
    except Exception as e:
        return f"写入文件失败: {e}"

@tool
def delete_file(file_path: str) -> str:
    """删除指定文件，传入 file_path"""
    try:
        os.remove(file_path)
        return f"文件 {file_path} 已删除"
    except Exception as e:
        return f"删除文件失败: {e}"

@tool
def run_shell_command(command: str) -> str:
    """在终端执行命令，并返回标准输出和错误，传入 command 字符串"""
    try:
        result = subprocess.run(command, shell=True, capture_output=True, text=True, timeout=30)
        return result.stdout + result.stderr
    except Exception as e:
        return f"执行命令失败: {e}"

@tool
def get_directory_contents(directory_path: str) -> str:
    """获取指定目录下的所有文件和子目录，传入 directory_path"""
    try:
        contents = os.listdir(directory_path)
        return contents
    except Exception as e:
        return f"获取目录内容失败: {e}"

from tool.session_memory_tool import search_session_memory

TOOLS = [read_file, write_file, delete_file, run_shell_command, get_directory_contents, search_session_memory]

def get_tool_list() -> str:
    """生成格式化的工具列表，供 LLM 理解可用工具"""
    result = []
    for tool in TOOLS:
        result.append(f"- **{tool.name}**: {tool.description}")
    tool_template = f"""
    ## 可用工具列表
    {result}
    """
    return tool_template
