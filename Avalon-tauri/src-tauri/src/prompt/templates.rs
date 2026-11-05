// 提示词内置常量模板
//
// 基本设定已迁入 soul 模块（作为 system 条目的默认内容，由 SoulRegistry 初始化时注入），
// 这里只保留压缩提示词模板（协议耦合，返回 CompressResult 的 JSON）。

/// 压缩层提示词：输入对话转录，输出结构化的 CompressResult。
pub fn build_compress_prompt(transcript: &str) -> (String, String) {
    let system = r#"这是一个压缩模型调用，用于把历史会话转录压缩成结构化摘要，返回纯 JSON。
样例 JSON 输出:
{
    "summary": ["被压缩会话的总结1", "被压缩会话的总结2"],
    "keywords": ["关键词1", "关键词2", "关键词3"],
    "facts": ["稳定事实，如用户身份、偏好、长期约束"],
    "decisions": ["已经做出的决定或结论"],
    "open_tasks": ["尚未完成、后续仍需要处理的事项"],
    "tool_outcomes": ["工具执行得到的关键结果或产物"],
    "time_range": "2026-11-01 ~ 2026-11-03"
}
要求：
- summary 是给模型读的叙述性总结，会被向量化用于语义检索；单条不超过 200 字，内容多时返回多条。
- facts / decisions / open_tasks / tool_outcomes 是稳定字段，没有内容时返回空数组。
- 不要编造转录里没有的信息；不要输出 JSON 以外的文字。
- 保留用户来源（飞书 / QQ / 桌面 / 定时任务）等归因信息。"#
        .to_string();

    let user = format!("压缩以下历史会话（对话转录）：\n{transcript}");
    (system, user)
}

/// 滚动摘要提示词：旧滚动状态 + 新压缩块 → 新的滚动状态。
///
/// `payload` 是 JSON：
/// ```json
/// {"old_rolling": {...} | null, "new_chunks": [{...}, ...]}
/// ```
pub fn build_rolling_prompt(payload: &str) -> (String, String) {
    let system = r#"这是一个滚动摘要更新调用，返回纯 JSON。
输入包含 old_rolling（已有滚动摘要，可能为 null）和 new_chunks（新压缩块的结构化摘要）。
请把两者合并成一份新的滚动摘要，结构固定为:
{
    "summary": ["叙述性总结"],
    "keywords": ["关键词"],
    "facts": ["稳定事实"],
    "decisions": ["已做出的决定"],
    "open_tasks": ["未完成任务"],
    "tool_outcomes": ["关键工具结果"],
    "time_range": "覆盖的时间范围"
}
要求：
- 这是「旧滚动摘要 + 新块摘要」的合并，不是对摘要再做一层摘要。
- 总长度控制在 1500 token 以内；超限时优先保留 open_tasks、decisions 和关键 facts。
- 合并重复信息，保留时间演进而非简单堆叠。
- 没有内容的字段返回空数组；不要输出 JSON 以外的文字。"#
        .to_string();

    let user = format!("更新滚动摘要：\n{payload}");
    (system, user)
}
