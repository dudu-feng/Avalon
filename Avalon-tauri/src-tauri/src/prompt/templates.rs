// 提示词内置常量模板
//
// 这些模板与对话层输出协议（llm/stream.rs 标记分隔三段式）、会话保存格式强耦合，
// 需求上不可自由编辑，故硬编码行内（非配置驱动）。action/compress 模板同理，
// 仍硬编码在 llm/client.rs，本期不迁移。

/// 基本设定（对应 Python assemble_system_prompt 首段，内容沿用）
pub const BASIC_SETTING: &str = r#"**基本设定**
你是智能体Avalon，是由 dudu-feng 开发的一款智能体，开发者对 Avalon 有以下期望：
- 为什么取 Avalon 这个名字：Avalon 是传说中遗世独立的理想乡，意在用户能在使用 Avalon 的过程中创造属于自己的智能体理想乡。
- "make your own Avalon"——"创造属于你自己的 Avalon""#;

/// 对话层标记协议约束（对应 Python response_template，协议改为标记分隔三段式）
///
/// 补齐 llm/stream.rs 缺失的「约束端」：StreamParser 按 <|thought|>/<|message|>/<|control|>
/// 三段标记解析，但此前没有任何提示词要求模型按此格式输出，导致整段被当正文、control
/// 解析不到、next 恒为 Stop。本模板告知模型输出格式。
pub const RESPONSE_TEMPLATE: &str = r#"请严格按照以下三段标记格式输出，不要省略任何标记：

<|thought|>
分析当前情况、规划下一步的思考过程
</|thought|>

<|message|>
给用户看的回复正文
</|message|>

<|control|>
{"next":"action 或 stop","action_target":"当 next=action 时需要执行的目标描述"}
</|control|>

规则：
- 三个标记块必须完整，缺一不可
- control 块内是纯 JSON，next 只能是 action 或 stop
- 不需要执行工具时 next=stop，可省略 action_target"#;

/// 动作层提示词（对应 Python llm_action 内联模板，从 client.rs 迁出）
/// 对齐 ActionResult { analysis, next, tool_call, sub_analysis }
pub fn build_action_prompt(action_target: &str, tool_list: &str, action_history: &str) -> String {
    format!(
        r#"这是一个action步骤模型调用，用于执行部分步式任务，请完成以下目标，当操作失败次数过多时，则停止执行操作：
{action_target}
遵守规则：
1. 拒绝发散性思考，只根据执行历史和工具列表进行分析。
2. 拒绝多次尝试同一错误操作，避免死循环。
3. 简洁思考，限制思考过程不要太长，保持思考效率。

返回纯JSON格式（不要用markdown代码块包裹）：
样例JSON输出:
{{
    "analysis": "分析当前情况，思考下一步应该做什么",
    "next": "tool_call / sub_analysis / finished",
    "tool_call": {{
        "name": "要调用的工具名称",
        "arguments": {{}}
    }},
    "sub_analysis": "子步骤分析/规划返回（仅next=sub_analysis时需要）"
}}

可用的工具列表：
{tool_list}

本次action步骤执行历史，当操作失败次数过多时，则停止执行操作:
{action_history}"#
    )
}

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
