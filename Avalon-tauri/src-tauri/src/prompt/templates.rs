// 提示词内置常量模板
//
// 基本设定与压缩提示词硬编码行内（非配置驱动）。工具调用协议由 OpenAI 原生 tools 参数承载，
// 不再需要三段标记模板约束模型输出格式。

/// 基本设定（对应 Python assemble_system_prompt 首段，内容沿用）
pub const BASIC_SETTING: &str = r#"**基本设定**
你是智能体Avalon，是由 dudu-feng 开发的一款智能体，开发者对 Avalon 有以下期望：
- 为什么取 Avalon 这个名字：Avalon 是传说中遗世独立的理想乡，意在用户能在使用 Avalon 的过程中创造属于自己的智能体理想乡。
- "make your own Avalon"——"创造属于你自己的 Avalon""#;

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
