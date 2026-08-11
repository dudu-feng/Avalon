"""
会话记忆查询工具

提供历史会话记忆的检索能力，支持三种搜索模式：
  - semantic: 语义向量检索（模糊回忆、同义改写）
  - keyword:  FTS 关键词全文检索（精确关键词查找）
  - hybrid:   混合检索（语义 + 关键词，默认推荐）

支持按时间范围过滤（基于 doc_id 中的时间戳）。
"""

import json
import os
from langchain_core.tools import tool
from loop.zvec_store import get_zvec_store
from config.env_config import env_config


@tool
def search_session_memory(
        query: str,
        search_mode: str = "hybrid",
        topk: int = 5,
        time_range: str = "",
    ) -> str:
    """
    搜索历史会话记忆。在压缩后的会话摘要中检索与 query 最相关的内容。

    参数说明：
    - query: 搜索文本，可以是自然语言描述（如"我们之前讨论过的压缩方案"）或关键词（如"Avalon 自我介绍"）
    - search_mode: 搜索模式，可选值：
        · "semantic" — 语义检索，适合模糊回忆、同义改写（如"退出"会找到"结束对话"相关内容）
        · "keyword"  — 关键词全文检索，适合精确查找（基于 jieba 中文分词）
        · "hybrid"   — 混合检索（默认推荐），兼顾语义和关键词匹配
    - topk: 返回结果数量，默认 5，范围 1-20
    - time_range: 时间过滤，留空表示不限制。格式：
        · 单个日期 "2026-07-15" → 查询该日期及之后的记忆
        · 日期区间 "2026-07-01,2026-07-31" → 查询该时间范围内的记忆

    返回格式：JSON 数组，每条结果包含：
    - location: 源会话片段的绝对路径（可直接用 read_file 读取原文，对应会话总体概要文件raw文件夹同级index.json文件）
    - session_id: 所属会话 ID
    - chunk: 压缩片段序号
    - description: 压缩摘要文本
    - keywords: 关键词列表
    - timestamp: 会话发生时间
    - score: 相关度分数（越高越相关）
    """
    # 参数校验
    search_mode = search_mode.strip().lower()
    if search_mode not in ("semantic", "keyword", "hybrid"):
        return json.dumps(
            {"error": f"不支持的搜索模式 '{search_mode}'，可选值: semantic, keyword, hybrid"},
            ensure_ascii=False,
        )

    topk = max(1, min(topk, 20))

    # 构建时间过滤表达式
    filter_expr = _build_time_filter(time_range)

    # 选择查询方法
    try:
        if search_mode == "semantic":
            results = get_zvec_store().vectorQuery_session_memory(query, topk, filter_expr)
        elif search_mode == "keyword":
            results = get_zvec_store().scalarQuery_session_memory(query, topk, filter_expr)
        else:  # hybrid
            results = get_zvec_store().hybridQuery_session_memory(query, topk, filter_expr)
    except Exception as e:
        return json.dumps(
            {"error": f"查询失败: {e}"},
            ensure_ascii=False,
        )

    # 格式化返回
    formatted = []
    for doc in results:
        doc_id = doc.id
        session_id, chunk = _parse_doc_id(doc_id)

        location = os.path.join(
            env_config.session_path, "history",
            session_id, "raw", f"{chunk}.json",
        )

        formatted.append({
            "location": location,
            "session_id": session_id,
            "chunk": chunk,
            "description": doc.fields.get("description", ""),
            "keywords": doc.fields.get("keyWords", []),
            "timestamp": doc.fields.get("timestamp", ""),
            "score": round(doc.score, 4) if doc.score else 0,
        })

    if not formatted:
        return json.dumps(
            {"message": "未找到相关会话记忆", "results": []},
            ensure_ascii=False,
        )

    return json.dumps(formatted, ensure_ascii=False, indent=2)


def _build_time_filter(time_range: str) -> str:
    """
    将用户输入的时间范围转为 ZVec filter 表达式。

    支持格式：
        ""                          → 无过滤
        "2026-07-15"                → timestamp >= "2026-07-15-00_00_00"
        "2026-07-01,2026-07-31"     → timestamp >= "..." && timestamp <= "..."
    """
    time_range = time_range.strip()
    if not time_range:
        return ""

    if "," in time_range:
        parts = time_range.split(",", 1)
        start = parts[0].strip()
        end = parts[1].strip()
        return f'timestamp >= "{start}-00_00_00" && timestamp <= "{end}-23_59_59"'
    else:
        return f'timestamp >= "{time_range}-00_00_00"'


def _parse_doc_id(doc_id: str) -> tuple:
    """
    从 doc_id 中解析会话 ID 和 chunk 标识。

    doc_id 格式:
      - 普通块:  {session_id}_chunk_{N}      例: "terminal_2026-06-06-23_03_31_chunk_2"
      - 合并块:  {session_id}_chunk_merged_{N} 例: "terminal_2026-06-06-23_03_31_chunk_merged_1"

    返回: (session_id, chunk_id)
      - 普通块 chunk_id 为 int（如 2）
      - 合并块 chunk_id 为 str（如 "merged_1"）
    """
    marker = "_chunk_"
    if marker in doc_id:
        idx = doc_id.rfind(marker)
        session_id = doc_id[:idx]
        chunk_str = doc_id[idx + len(marker):]
        # 优先尝试 int 解析（普通块）
        try:
            chunk = int(chunk_str)
        except ValueError:
            # 合并块，保留原始字符串
            chunk = chunk_str
        return session_id, chunk
    return doc_id, 0
