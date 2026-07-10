from langchain_core.prompts import ChatPromptTemplate
from langchain_core.prompts.chat import MessagesPlaceholder
from langchain_core.messages import SystemMessage, HumanMessage, AIMessage
import os

from config.env_config import env_config

# 优先使用 .env 配置的绝对路径，否则回退到相对路径
prompt_file_path = env_config.prompt_file_path
if not os.path.isdir(prompt_file_path):
    # 回退：基于当前文件位置向上两级查找 config/prompt
    current_dir = os.path.dirname(os.path.abspath(__file__))
    agent_dir = os.path.dirname(current_dir)
    prompt_file_path = os.path.join(agent_dir, "config", "prompt")

# 获取提示词
def load_prompt( file_name: str ) -> str:
  file_path = os.path.join(prompt_file_path, file_name)
  with open(file_path, 'r', encoding='utf-8') as f:
    return f.read().strip()

def assemble_system_prompt() -> list:
    # 查看目录下所有markdown文件
    file_list = [f for f in os.listdir(prompt_file_path) if f.endswith(".md")]
    # 遍历所有文件，加载提示词
    prompt_list = ["""
        **基本设定**
        你是智能体Avalon,是由dudu-feng开发的一款智能体，开发者对Avalon这个智能体由以下期望：
        - 为什么取Avalon这个名字，Avalon是传说中遗世独立的理想乡，意在用户能在使用Avalon的过程中创造属于你自己的智能体理想乡
        ”make your own Avalon“——”创造属于你自己的Avalon“
    """]
    for file_name in file_list:
        prompt = load_prompt(file_name)
        prompt_list.append(prompt)

    return prompt_list
