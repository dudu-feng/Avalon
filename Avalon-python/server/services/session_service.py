"""
会话管理服务 —— 自包含多会话逻辑

在 agent/loop/session_manage.py 的基础上，
直接实现多会话的列表、详情、创建、压缩、归档、删除功能。
不修改 agent/ 层代码。
"""

import json
import os
import shutil
from datetime import datetime

from config.env_config import env_config
from loop import session_manage
from server.core.exceptions import (
    ActiveSessionDeleteForbiddenException,
    SessionEmptyForCompressException,
    SessionNotFoundException,
)

# ============================================================
#  路径工具
# ============================================================

SESSION_FILE = "terminal.json"


def _current_file_path() -> str:
    return os.path.join(env_config.session_path, "current", SESSION_FILE)


def _history_dir() -> str:
    return os.path.join(env_config.session_path, "history", "terminal")


def _history_session_path(session_id: str) -> str:
    return os.path.join(_history_dir(), session_id)


# ============================================================
#  辅助函数
# ============================================================

def _session_summary(data: dict) -> dict:
    """从会话数据中提取摘要信息"""
    session_list = data.get("session", [])
    last_time = session_list[-1]["time"] if session_list else None
    return {
        "id": data.get("id", ""),
        "status": data.get("status", "inactive"),
        "compress_round": data.get("compress_round", 0),
        "message_count": len(session_list),
        "last_message_time": last_time,
    }


def _read_json(path: str) -> dict | None:
    """安全读取 JSON 文件"""
    try:
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f)
    except Exception:
        return None


def _walk_count(directory: str) -> int:
    """目录下文件总数"""
    count = 0
    for _root, _dirs, files in os.walk(directory):
        count += len(files)
    return count


# ============================================================
#  公开接口
# ============================================================


def list_sessions() -> dict:
    """列出所有会话（当前 + 历史）"""
    result = {"current": None, "history": []}

    # 当前活跃会话
    cf = _current_file_path()
    if os.path.exists(cf):
        data = _read_json(cf)
        if data and data.get("status") == "active":
            result["current"] = _session_summary(data)

    # 历史归档会话
    hd = _history_dir()
    if os.path.isdir(hd):
        try:
            for name in sorted(os.listdir(hd), reverse=True):
                ip = os.path.join(hd, name, "index.json")
                if os.path.isfile(ip):
                    data = _read_json(ip)
                    if data:
                        result["history"].append(_session_summary(data))
        except Exception:
            pass

    return result


def get_session(session_id: str) -> dict:
    """获取会话完整数据（先查 current，再查 history）"""
    # current
    cf = _current_file_path()
    data = _read_json(cf)
    if data and data.get("id") == session_id:
        return data

    # history
    ip = os.path.join(_history_session_path(session_id), "index.json")
    data = _read_json(ip)
    if data:
        return data

    raise SessionNotFoundException(f"会话 {session_id} 不存在")


def create_session(preserve_current: bool = True) -> dict:
    """创建新会话"""
    if preserve_current:
        cf = _current_file_path()
        data = _read_json(cf)
        if data and data.get("status") == "active" and data.get("session"):
            session_manage.session_compress()
            session_manage.save_current_session()

    timestamp = datetime.now().strftime("%Y-%m-%d-%H_%M_%S")
    session_id = f"terminal_{timestamp}"
    new_session = {
        "id": session_id,
        "status": "active",
        "compress_round": 0,
        "compressed": [],
        "super_compressed": [],
        "session": [],
    }
    os.makedirs(os.path.dirname(_current_file_path()), exist_ok=True)
    with open(_current_file_path(), "w", encoding="utf-8") as f:
        json.dump(new_session, f, ensure_ascii=False, indent=2)

    return {
        "id": session_id,
        "status": "active",
        "created_at": datetime.now().strftime("%Y-%m-%d-%H:%M:%S"),
    }


def compress_session(session_id: str) -> dict:
    """压缩会话"""
    data = get_session(session_id)

    if data.get("status") != "active":
        raise SessionNotFoundException("只有活跃会话可以压缩")
    if not data.get("session"):
        raise SessionEmptyForCompressException()

    before_round = data.get("compress_round", 0)
    before_count = len(data.get("session", []))

    session_manage.session_compress()

    data_after = _read_json(_current_file_path()) or {}
    after_round = data_after.get("compress_round", 0)
    compressed = data_after.get("compressed", [])
    latest_chunk = compressed[-1] if compressed else {}

    return {
        "session_id": session_id,
        "compress_round": after_round,
        "archived_messages": before_count,
        "chunk_summary": latest_chunk.get("summary", []),
        "chunk_keywords": latest_chunk.get("keywords", []),
        "progressive_merged": after_round > before_round + 1,
    }


def archive_session(session_id: str) -> dict:
    """归档活跃会话"""
    cf = _current_file_path()
    data = _read_json(cf)

    if not data:
        raise SessionNotFoundException("当前没有活跃会话")
    if data.get("id") != session_id:
        raise SessionNotFoundException(f"会话 {session_id} 不是当前活跃会话")
    if data.get("status") != "active":
        raise SessionNotFoundException(f"会话 {session_id} 已经归档")

    session_manage.session_compress()
    session_manage.save_current_session()

    return {
        "session_id": session_id,
        "archived_at": datetime.now().strftime("%Y-%m-%d-%H:%M:%S"),
    }


def delete_session(session_id: str) -> dict:
    """删除历史会话（含 ZVec 向量清理）"""
    # 安全检查：不能删活跃会话
    cf = _current_file_path()
    data = _read_json(cf)
    if data and data.get("id") == session_id and data.get("status") == "active":
        raise ActiveSessionDeleteForbiddenException()

    # 删除历史目录
    sd = _history_session_path(session_id)
    if not os.path.isdir(sd):
        raise SessionNotFoundException(f"会话 {session_id} 不存在")

    # ZVec 清理
    zvec_removed = 0
    ip = os.path.join(sd, "index.json")
    hist_data = _read_json(ip)
    if hist_data:
        chunks = hist_data.get("compressed", [])
        sc = hist_data.get("super_compressed")
        if sc:
            chunks.append(sc)
        try:
            from loop.zvec_store import zvec_store

            for c in chunks:
                cid = c.get("chunk", 0)
                if cid:
                    try:
                        zvec_store.delete_session_memory(
                            f"{session_id}_chunk_{cid}"
                        )
                        zvec_removed += 1
                    except Exception:
                        pass
        except Exception:
            pass

    deleted = _walk_count(sd)
    shutil.rmtree(sd, ignore_errors=True)

    return {
        "session_id": session_id,
        "deleted_files": deleted,
        "zvec_entries_removed": zvec_removed,
    }


def get_raw_chunk(session_id: str, chunk: str) -> dict:
    """读取压缩块原始对话"""
    raw_path = os.path.join(
        _history_session_path(session_id), "raw", f"{chunk}.json"
    )

    data = _read_json(raw_path)
    if data is None:
        raise SessionNotFoundException(
            f"chunk {chunk} 不存在于会话 {session_id}"
        )

    chunk_type = (
        "merged_summary" if str(chunk).startswith("merged") else "compressed_chunk"
    )

    return {
        "session_id": session_id,
        "chunk": chunk,
        "type": chunk_type,
        "messages": data if isinstance(data, list) else [data],
    }
