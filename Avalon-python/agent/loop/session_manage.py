import os
import json
import shutil
from datetime import datetime

try:
    from dotenv import load_dotenv
    load_dotenv()
except ImportError:
    pass

session_path = os.getenv("session_path", "data/session")
session_index_path = os.getenv("session_index_path", "data/session/index.json")


def update_current_session(chat_history: list):
    """
    更新当前会话历史记录到 session_path/current/terminal.json
    """
    current_dir = os.path.join(session_path, "current")
    os.makedirs(current_dir, exist_ok=True)

    file_path = os.path.join(current_dir, "terminal.json")
    with open(file_path, 'w', encoding='utf-8') as f:
        json.dump(chat_history, f, ensure_ascii=False, indent=2)


def save_current_session():
    """
    保存当前会话历史记录到历史会话目录
    session_path/history/terminal/terminal_{timestamp}.json
    """
    current_file = os.path.join(session_path, "current", "terminal.json")
    if not os.path.exists(current_file):
        print("⚠️ 没有当前会话记录可保存")
        return None

    history_dir = os.path.join(session_path, "history", "terminal")
    os.makedirs(history_dir, exist_ok=True)

    timestamp = datetime.now().strftime("%Y-%m-%d-%H%M%S")
    history_file = os.path.join(history_dir, f"terminal_{timestamp}.json")

    shutil.copy2(current_file, history_file)
    print(f"✅ 会话已归档: {history_file}")

    # 更新索引
    _update_index(timestamp, history_file)

    return history_file


# ═══════════════════════════════════════════════════════════
# 内部辅助
# ═══════════════════════════════════════════════════════════

def _update_index(session_id: str, file_path: str):
    """在 index.json 中追加一条会话记录"""
    if not session_index_path:
        return

    os.makedirs(os.path.dirname(session_index_path), exist_ok=True)

    # 读取现有索引
    index = []
    if os.path.exists(session_index_path):
        try:
            with open(session_index_path, 'r', encoding='utf-8') as f:
                index = json.load(f)
        except (json.JSONDecodeError, FileNotFoundError):
            index = []

    # 尝试读取会话文件提取标题
    title = "会话"
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            msgs = json.load(f)
            for m in msgs:
                if m.get("role") == "user":
                    raw = m.get("content", "")
                    title = raw[:40] + ("..." if len(raw) > 40 else "")
                    break
    except Exception:
        pass

    index.insert(0, {
        "session_id": session_id,
        "title": title,
        "saved_at": datetime.now().strftime("%Y-%m-%d %H:%M:%S"),
    })

    with open(session_index_path, 'w', encoding='utf-8') as f:
        json.dump(index, f, ensure_ascii=False, indent=2)
