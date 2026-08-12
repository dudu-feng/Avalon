from langchain_core.prompts import ChatPromptTemplate
from langchain_core.prompts.chat import MessagesPlaceholder
from langchain_core.messages import SystemMessage, HumanMessage, AIMessage
import os

from config.env_config import env_config

prompt_file_path = env_config.prompt_file_path
if not os.path.isdir(prompt_file_path):
    raise FileNotFoundError(f"提示词目录不存在: {prompt_file_path}，请检查 .env 中 prompt_file_path 配置")

# ========== 提示词缓存 ==========
_cached_prompt_list: list | None = None


def load_prompt(file_name: str) -> str:
    """读取单个提示词文件"""
    file_path = os.path.join(prompt_file_path, file_name)
    with open(file_path, 'r', encoding='utf-8') as f:
        return f.read().strip()


def assemble_system_prompt() -> list:
    """组装系统提示词列表（首次加载后缓存，后续调用返回副本）"""
    global _cached_prompt_list
    if _cached_prompt_list is None:
        file_list = [f for f in os.listdir(prompt_file_path) if f.endswith(".md")]
        prompt_list = ["""
            **基本设定**
            你是智能体Avalon,是由dudu-feng开发的一款智能体，开发者对Avalon这个智能体由以下期望：
            - 为什么取Avalon这个名字，Avalon是传说中遗世独立的理想乡，意在用户能在使用Avalon的过程中创造属于你自己的智能体理想乡
            "make your own Avalon"——"创造属于你自己的Avalon"
        """]
        for file_name in file_list:
            prompt_list.append(load_prompt(file_name))
        _cached_prompt_list = prompt_list

    # 返回副本，防止调用方 append() 污染缓存
    return list(_cached_prompt_list)


def refresh_prompt_cache():
    """清空提示词缓存（修改 prompt 文件后调用，下次组装时自动重新加载）"""
    global _cached_prompt_list
    _cached_prompt_list = None
