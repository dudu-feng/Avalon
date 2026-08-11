import os
import json
import shutil
from datetime import datetime
from config.env_config import env_config
from llm import llm
from loop import react_loop
from loop.zvec_store import zvec_store

session_path = env_config.session_path
session_index_path = env_config.session_index_path


def _current_file(channel: str) -> str:
    """构建当前会话文件路径: session_path/current/{channel}.json"""
    return os.path.join(session_path, "current", f"{channel}.json")


def init_session(channel: str = "terminal"):
    """
    初始化当前会话
    session_path/current/{channel}.json
    """
    current_file = _current_file(channel)
    try:
        with open(current_file, 'r', encoding='utf-8') as f:
            data = json.load(f)
    except FileNotFoundError:
        data = {}
    if data.get("status") == "active":
        print("已识别到上次会话，继续上次对话。")
        return
    else:
        timestamp = datetime.now().strftime("%Y-%m-%d-%H_%M_%S")
        data["id"] = f"{channel}_{timestamp}"
        data["status"] = "active"
        data["compress_round"] = 0
        data["compressed"] = []
        data["super_compressed"] = []
        data["session"] = []
        with open(current_file, 'w', encoding='utf-8') as f:
            json.dump(data, f, ensure_ascii=False, indent=2)


def get_current_session(channel: str = "terminal"):
    current_file = _current_file(channel)
    with open(current_file, 'r', encoding='utf-8') as f:
        data = json.load(f)
    return data


def update_current_session(chat_history: list, channel: str = "terminal"):
    """
    更新当前会话历史记录到 session_path/current/{channel}.json
    """
    current_file = _current_file(channel)
    with open(current_file, 'r', encoding='utf-8') as f:
        data = json.load(f)
    data["session"].extend(chat_history)
    with open(current_file, 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, indent=2)


def save_current_session(channel: str = "terminal"):
    """
    保存当前会话历史记录到历史会话目录
    session_path/history
    """
    session_compress(channel)
    current_file = _current_file(channel)
    history_dir = os.path.join(session_path, "history")
    with open(current_file, 'r', encoding='utf-8') as f:
        data = json.load(f)
    data["status"] = "archived"
    os.makedirs(os.path.join(history_dir, data["id"]), exist_ok=True)
    with open(os.path.join(history_dir, data["id"], "index.json"), 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
    data = {
        "id": "",
        "status": "inactive",
        "compress_round": 0,
        "compressed": [],
        "super_compressed": [],
        "session": []
    }
    with open(current_file, 'w', encoding='utf-8') as f:
        json.dump(data, f, ensure_ascii=False, indent=2)


def session_compress(channel: str = "terminal"):
    """
    压缩历史会话目录
    session_path/history
    """
    history_dir = os.path.join(session_path, "history")
    current_file = _current_file(channel)
    current_session = get_current_session(channel)
    if not current_session["session"]:
        print("当前会话为空，无需压缩。")
        return
    compressed_data = llm.llm_compress(current_session)
    compressed_data_content = react_loop.parse_llm_json(compressed_data.content)
    if not compressed_data_content:
        print("压缩模型返回的 JSON 内容无法解析，无法继续压缩。")
        return
    current_session["compress_round"] = current_session.get("compress_round", 0) + 1
    compressed_data_content["chunk"] = current_session["compress_round"]
    current_session["compressed"].append(compressed_data_content)
    raw_dir = os.path.join(history_dir, current_session["id"], "raw")
    os.makedirs(raw_dir, exist_ok=True)
    with open(os.path.join(raw_dir, f"{compressed_data_content['chunk']}.json"), 'w', encoding='utf-8') as f:
        json.dump(current_session["session"], f, ensure_ascii=False, indent=2)
    current_session["session"] = []
    with open(current_file, 'w', encoding='utf-8') as f:
        json.dump(current_session, f, ensure_ascii=False, indent=2)
    print(f"当前会话已压缩。{len(current_session['compressed'])}个压缩记录")

    # -- 将压缩摘要同步写入向量数据库 --
    # doc_id 格式: {会话ID}_chunk_{序号}，后续可通过 ID 反查源会话文件
    try:
        doc_id = f"{current_session['id']}_chunk_{compressed_data_content['chunk']}"
        # 从 session_id 提取时间戳：去掉渠道前缀 "{channel}_"
        timestamp = current_session['id'].replace(f"{channel}_", "", 1)
        summary_text = "\n".join(compressed_data_content["summary"])
        keywords = compressed_data_content["keywords"]
        zvec_store.insert_session_memory(doc_id, summary_text, keywords, timestamp)
        print(f"[ZVec] 会话摘要已写入向量数据库: {doc_id}")
    except Exception as e:
        print(f"[ZVec] 写入向量数据库失败（不影响压缩流程）: {e}")

    # -- 永恒会话：检查是否需要渐进式总结（旧块合并） --
    _progressive_summarize(channel)


# ============================================================
#  自动压缩 & 永恒会话管理
# ============================================================

def auto_compress_check_from_history(chat_history: list, channel: str = "terminal") -> bool:
    """
    从 chat_history 中提取最大 input_tokens，判断是否触发自动压缩。

    应在 update_current_session() 之后调用，确保最新消息已持久化。

    返回:
        True   — 已触发压缩
        False  — 未达到阈值，跳过
    """
    # 遍历 chat_history 及嵌套的 action_history，收集所有 input_tokens
    max_input = 0

    def _collect_tokens(entries: list):
        nonlocal max_input
        for entry in entries:
            if not isinstance(entry, dict):
                continue
            token_usage = entry.get("token_usage", {})
            input_tokens = token_usage.get("input_tokens", 0)
            if input_tokens > max_input:
                max_input = input_tokens
            # 递归检查 action_history
            nested = entry.get("action_history", [])
            if nested:
                _collect_tokens(nested)

    _collect_tokens(chat_history)

    threshold = env_config.session_memory_compress_threshold
    if max_input >= threshold:
        print(f"[AutoCompress] 输入 token({max_input}) >= 阈值({threshold})，触发自动压缩...")
        session_compress(channel)
        return True

    return False


def get_session_context_for_prompt(channel: str = "terminal") -> dict:
    """
    返回限界会话上下文，用于 LLM 系统提示。

    策略：
      - super_compressed（历史超级摘要）始终包含，代表完整历史
      - compressed 只包含最近 N 个普通压缩块（session_memory_context_chunks，默认5）
      - 旧块不加载到上下文，由 search_session_memory 工具按需检索
      - 避免永恒会话场景下系统提示无限膨胀

    返回:
        dict — 裁剪后的会话数据（浅拷贝，不影响文件中的完整数据）
    """
    data = get_current_session(channel)
    max_context = env_config.session_memory_context_chunks
    compressed = data.get("compressed", [])

    if len(compressed) <= max_context:
        return data

    omitted = len(compressed) - max_context
    bounded = {
        "id": data["id"],
        "status": data["status"],
        "super_compressed": data.get("super_compressed"),
        "compressed": compressed[-max_context:],
        "session": data["session"],
        "_older_chunks_omitted": omitted,
    }
    return bounded


def _progressive_summarize(channel: str = "terminal"):
    """
    永恒会话处理：当普通压缩块超过上限时，将最旧块 + 历史超级摘要合并。

    合并策略（递归合并，抑制无限膨胀）:
      - super_compressed（历史超级摘要）独立存储，不占 compressed 槽位
      - 每次固定取 merge_batch（= max_chunks // 2，默认 5）个逻辑块
      - 若 super_compressed 已存在，将其作为最旧块纳入合并
      - 命名: merged_{start}_{end}，反映覆盖的原始块范围
      - 结果: super_compressed 永远只有 1 个，compressed 稳定在 ~6 个普通块

    示例（max_chunks=10, merge_batch=5）:
      第1次触发: [1,2,3,4,5]                         → super_compressed=merged_1_5
      第2次触发: super_compressed + [6,7,8,9]       → super_compressed=merged_1_9
      第3次触发: super_compressed + [10,11,12,13]   → super_compressed=merged_1_13
      ...永不膨胀

    触发条件:
      len(compressed) > session_memory_max_chunks（默认 10）
    """
    max_chunks = env_config.session_memory_max_chunks
    merge_batch = max_chunks // 2  # 固定每次合并 5 个逻辑块

    current = get_current_session(channel)
    compressed = current.get("compressed", [])
    super_compressed = current.get("super_compressed")

    # 只用 compressed 普通块数量判断（super_compressed 不占槽位）
    if len(compressed) <= max_chunks:
        return

    # --- 构建待合并列表：历史超级摘要（若存在）视为最旧逻辑块 ---
    if super_compressed:
        old_chunks = [super_compressed] + compressed[:merge_batch - 1]
        recent_chunks = compressed[merge_batch - 1:]
    else:
        old_chunks = compressed[:merge_batch]
        recent_chunks = compressed[merge_batch:]

    # --- 收集摘要、关键词、块范围 ---
    all_summaries = []
    all_keywords = []
    raw_chunk_nums = []

    def _extract_nums(chunk_entry: dict) -> list:
        """从任意块中提取原始块编号列表"""
        chk = chunk_entry.get("chunk")
        nums = []
        if isinstance(chk, int):
            nums.append(chk)
        elif isinstance(chk, str) and chk.startswith("merged"):
            parts = chk.replace("merged_", "").split("_")
            for p in parts:
                try:
                    nums.append(int(p))
                except ValueError:
                    pass
            for mf in chunk_entry.get("merged_from_chunks", []):
                if isinstance(mf, int):
                    nums.append(mf)
                elif isinstance(mf, str) and mf.startswith("merged"):
                    for pp in mf.replace("merged_", "").split("_"):
                        try:
                            nums.append(int(pp))
                        except ValueError:
                            pass
        return nums

    for chunk in old_chunks:
        all_summaries.extend(chunk.get("summary", []))
        all_keywords.extend(chunk.get("keywords", []))
        raw_chunk_nums.extend(_extract_nums(chunk))

    start_num = min(raw_chunk_nums) if raw_chunk_nums else 1
    end_num = max(raw_chunk_nums) if raw_chunk_nums else merge_batch
    merged_chunk_num = f"merged_{start_num}_{end_num}"

    # --- 二次压缩 ---
    mock_session = {
        "session": [
            {"role": "assistant", "content": s} for s in all_summaries
        ]
    }

    try:
        merged_result = llm.llm_compress(mock_session)
        merged_content = react_loop.parse_llm_json(merged_result.content)
    except Exception as e:
        print(f"[ProgressiveSummarize] 合并压缩调用失败: {e}")
        return

    if not merged_content:
        print("[ProgressiveSummarize] 合并压缩返回空，使用简单拼接降级")
        merged_content = {
            "summary": all_summaries[:3],
            "keywords": list(set(all_keywords))[:10],
        }

    session_id = current["id"]

    merged_from_ids = [c.get("chunk") for c in old_chunks]

    merged_chunk = {
        "chunk": merged_chunk_num,
        "summary": merged_content.get("summary", all_summaries[:3]),
        "keywords": merged_content.get("keywords", list(set(all_keywords))[:10]),
        "merged_from_chunks": merged_from_ids,
    }

    # --- 更新：super_compressed 独立存储，compressed 只保留普通块 ---
    current["super_compressed"] = merged_chunk
    current["compressed"] = recent_chunks
    current_file = _current_file(channel)
    with open(current_file, 'w', encoding='utf-8') as f:
        json.dump(current, f, ensure_ascii=False, indent=2)

    # --- 同步 ZVec：删除旧块（含旧的 super_compressed） ---
    for old in old_chunks:
        old_num = old.get("chunk", 0)
        try:
            zvec_store.delete_session_memory(f"{session_id}_chunk_{old_num}")
        except Exception:
            pass

    # --- 同步 ZVec：插入超级摘要 ---
    try:
        merged_doc_id = f"{session_id}_chunk_{merged_chunk_num}"
        merged_text = "\n".join(merged_chunk["summary"])
        merged_keywords = merged_chunk["keywords"]
        # 从 session_id 提取时间戳：去掉渠道前缀 "{channel}_"
        timestamp = session_id.replace(f"{channel}_", "", 1)
        zvec_store.insert_session_memory(merged_doc_id, merged_text, merged_keywords, timestamp)
        print(f"[ZVec] 渐进式总结已写入: {merged_doc_id}")
    except Exception as e:
        print(f"[ProgressiveSummarize] ZVec 写入失败: {e}")

    # --- 保存 raw 文件 ---
    try:
        raw_dir = os.path.join(session_path, "history", session_id, "raw")
        os.makedirs(raw_dir, exist_ok=True)
        with open(os.path.join(raw_dir, f"{merged_chunk_num}.json"), 'w', encoding='utf-8') as f:
            json.dump({
                "type": "merged_summary",
                "merged_from_chunks": merged_chunk["merged_from_chunks"],
                "summary": merged_chunk["summary"],
                "keywords": merged_chunk["keywords"],
            }, f, ensure_ascii=False, indent=2)
    except Exception as e:
        print(f"[ProgressiveSummarize] raw 文件保存失败: {e}")

    print(
        f"[ProgressiveSummarize] {len(old_chunks)} 个逻辑块 → 1 个超级摘要 {merged_chunk_num} "
        f"(覆盖原始块 {start_num}~{end_num}，compressed 剩余 {len(current['compressed'])} 块)"
    )
