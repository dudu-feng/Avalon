import os
from typing import Optional
from langchain_openai import ChatOpenAI
from langchain_core.prompts import ChatPromptTemplate
from langchain_core.output_parsers import StrOutputParser
from langchain_core.messages import BaseMessage, HumanMessage, SystemMessage, AIMessage

from agent.loop import prompt_assemble
from agent.tool import get_tool_list

try:
    from dotenv import load_dotenv
    load_dotenv()
except ImportError:
    pass

model_type = "default"

response_template = f"""
    请按照以下格式进行思考和回应：

    - 分析当前情况，思考下一步应该做什么
    - 如果需要执行工具，根据现有工具列表规划工具调用
    
    返回格式如下：
    {
        "thought": str, (分析当前情况，思考下一步应该做什么)
        "message": str, (输出给用户看的消息)
        "next": str, (下一步应该做什么action/stop)
        "action_target": str, (需要分步执行的action的目标，只传下一步需要执行的action目标，将传给子模型执行action)
    }
"""


# 获取环境变量
def get_model() -> ChatOpenAI:
    return ChatOpenAI(
        api_key=os.getenv(f"{model_type}_api_key"),
        model_name=os.getenv(f"{model_type}_model"),
        base_url=os.getenv(f"{model_type}_model_base_url"),
    )

# 切换模型类型
def change_model_type( type: str ):
    global model_type
    model_type = type

def llm_chat( user_input: str ):
    model = get_model()
    system_prompt = prompt_assemble.assemble_system_prompt()
    tool_list = get_tool_list()

    system_prompt.append(response_template)
    system_prompt.append(tool_list)

    # 构造提示模板
    prompt_template = ChatPromptTemplate.from_messages([
        ("system", "\n".join(system_prompt)),
        ("user", user_input)
    ])

    # 生成消息
    messages = prompt_template.format_messages()

    # 调用模型
    result = model.invoke(messages)

    return result

def llm_action( action_target: str ):
    model = get_model()
    system_prompt = f"""
        这是一个action步骤模型调用，用于执行部分步式任务，请完成以下目标：
        {action_target}
        返回格式如下：
        {
            "analysis": str, (分析当前情况，思考下一步应该做什么)
            "next": str, (下一步应该做什么tool_call/sub_analysis/finished)
            "tool_call": {
                "name": str, (要调用的工具名称)
                "arguments": object, (要传递给工具的参数)
            },
            "sub_analysis_return": str, (子步骤分析/规划返回)
        }
    """
    tool_list = get_tool_list()
    system_prompt.append(tool_list)

    prompt_template = ChatPromptTemplate.from_messages([
        ("system", system_prompt),
        ("human", f"请根据以上指令完成任务目标{action_target}，并按照指定格式输出结果。")
    ])

    # 生成消息
    messages = prompt_template.format_messages()

    # 调用模型
    result = model.invoke(messages)

    return result
