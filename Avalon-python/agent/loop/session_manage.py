import os
import json
import shutil
from datetime import datetime
from llm import llm
from loop import react_loop

try:
    from dotenv import load_dotenv
    load_dotenv()
except ImportError:
    print("⚠️ 未找到 .env 文件，将使用默认配置")
    pass

session_path = os.getenv("session_path", "data\memory\session")
session_index_path = os.getenv("session_index_path", "data\memory\session\index.json")

def init_session():
    """
    初始化当前会话
    session_path/current/
    """
    current_file = os.path.join(session_path, "current", "terminal.json")
    try:
        with open(current_file, 'r', encoding='utf-8') as f:
            data = json.load(f)
    except FileNotFoundError:
        data = {}
    if data["status"] == "active":
        print("已识别到上次会话，继续上次对话。")
        return
    else:
        timestamp = datetime.now().strftime("%Y-%m-%d-%H_%M_%S")
        data["id"] = f"terminal_{timestamp}"
        data["status"] = "active"
        data["compressed"] = []
        data["session"] = []
        with open(current_file, 'w', encoding='utf-8') as f:
            json.dump(data, f, ensure_ascii=False, indent=2)

def get_current_session():
    current_file = os.path.join(session_path, "current", "terminal.json")
    with open(current_file, 'r', encoding='utf-8') as f:
        data = json.load(f)
    return data

def update_current_session(chat_history: list):
    """
    更新当前会话历史记录到 session_path/current/terminal.json
    """
    current_file = os.path.join(session_path, "current", "terminal.json")
    with open(current_file, 'r', encoding='utf-8') as f:
        data = json.load(f)
    data["session"].extend(chat_history)
    with open(current_file, 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, indent=2)

def save_current_session():
    """
    保存当前会话历史记录到历史会话目录
    session_path/history/terminal
    """
    session_compress()
    current_file = os.path.join(session_path, "current", "terminal.json")
    history_dir = os.path.join(session_path, "history", "terminal")
    with open(current_file, 'r', encoding='utf-8') as f:
        data = json.load(f)
    data["status"] = "archived"
    with open(os.path.join(history_dir, data["id"], "index.json"), 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
    data = {
        "id": "",
        "status": "inactive",
        "compressed":[],
        "session": []
    }
    with open(current_file, 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, indent=2)

def session_compress():
    """
    压缩历史会话目录
    session_path/history/terminal
    """
    history_dir = os.path.join(session_path, "history", "terminal")
    current_file = os.path.join(session_path, "current", "terminal.json")
    current_session = get_current_session()
    if not current_session["session"]:
        print("当前会话为空，无需压缩。")
        return
    compressed_data = llm.llm_compress(current_session)
    compressed_data_content = react_loop.parse_llm_json(compressed_data.content)
    if not compressed_data_content:
        print("压缩模型返回的 JSON 内容无法解析，无法继续压缩。")
        return
    compressed_data_content["chunk"] = len(current_session["compressed"]) + 1
    current_session["compressed"].append(compressed_data_content)
    raw_dir = os.path.join(history_dir, current_session["id"], "raw")
    os.makedirs(raw_dir, exist_ok=True)
    with open(os.path.join(raw_dir, f"{compressed_data_content['chunk']}.json"), 'w', encoding='utf-8') as f:
        json.dump(current_session["session"], f, ensure_ascii=False, indent=2)
    current_session["session"] = []
    with open(current_file, 'w', encoding='utf-8') as f:
        json.dump(current_session, f, ensure_ascii=False, indent=2)
    print(f"当前会话已压缩。{len(current_session['compressed'])}个压缩记录")
