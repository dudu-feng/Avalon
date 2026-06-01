from langchain_core.prompts import ChatPromptTemplate
from langchain_core.prompts.chat import MessagesPlaceholder
from langchain_core.messages import SystemMessage, HumanMessage, AIMessage
import os

prompt_file_path = os.getenv("prompt_file_path")

# 获取提示词
def load_prompt( file_name: str ) -> str:
  file_path = os.path.join(prompt_file_path, file_name)
  with open(file_path, 'r', encoding='utf-8') as f:
    return f.read().strip()

def assemble_system_prompt() -> list:
    # 查看目录下所有markdown文件
    file_list = [f for f in os.listdir(prompt_file_path) if f.endswith(".md")]
    # 遍历所有文件，加载提示词
    prompt_list = []
    for file_name in file_list:
        prompt = load_prompt(file_name)
        prompt_list.append(prompt)
  
    return prompt_list