// 提示词内置常量模板
//
// 基本设定已迁入 soul 模块（作为 system 条目的默认内容，由 SoulRegistry 初始化时注入），
// 这里只保留压缩提示词模板（协议耦合，返回 CompressResult { summary, keywords } 的 JSON）。

/// 压缩层提示词（对应 Python llm_compress 内联模板，从 client.rs 迁出）
/// 对齐 CompressResult { summary, keywords }，返回 (system, user)
pub fn build_compress_prompt(session_data: &str) -> (String, String) {
    let system = r#"这是一个压缩模型调用，用于压缩历史会话记录，返回纯JSON格式。
样例JSON输出:
{
    "summary": ["被压缩会话的总结1", "被压缩会话的总结2"],
    "keywords": ["关键词1", "关键词2", "关键词3"]
}
注意：summary 是被压缩会话的总结，后续会向量化作会话语义检索，单个总结长度不超过200个字符，会话内容较多时可返回多个总结。
keywords 是被压缩会话内容的精炼关键词，用于关键词检索，可以是概括性关键词，也可以是重要事件、关键事物的指向性关键词。"#
        .to_string();

    let user = format!("压缩以下历史会话：\n{session_data}");
    (system, user)
}
