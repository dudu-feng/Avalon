// 技能工具 schema。

use serde_json::json;

use super::ToolDef;

/// 技能工具定义（仅注入 SkillRegistry 时暴露，5 个）
pub(crate) fn skill_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "use_skill",
            description: "加载指定技能的流程指令。技能清单（name + description）见系统提示词里的「可用技能」段；\
                          当任务匹配某个技能时，先调用本工具获取其详细步骤，再按步骤执行。\
                          正文末尾会列出该技能的参考文档（references），需要时再带 reference 参数逐份读取"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "技能名（「可用技能」清单里的 name）"},
                    "reference": {"type": "string", "description": "可选：要加载的二级参考文档相对路径（如 auth.md 或 sub/advanced.md），省略则返回技能正文"}
                },
                "required": ["name"]
            }),
        },
        ToolDef {
            name: "create_skill",
            description: "沉淀一个新技能（可复用流程知识）。写技能有门槛：仅当某类任务反复出现、\
                          且步骤可复用、确实值得沉淀时才创建；创建前先对照系统提示词里的「可用技能」清单查重。\
                          name 只能含字母/数字/-/_，description 一句话说明何时用，content 为可执行的流程步骤"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "技能名（目录名，仅字母/数字/-/_）"},
                    "description": {"type": "string", "description": "一句话描述这个技能做什么、何时用"},
                    "content": {"type": "string", "description": "技能正文：可执行的流程步骤"}
                },
                "required": ["name", "content"]
            }),
        },
        ToolDef {
            name: "update_skill",
            description: "修改已有技能的描述或正文（name 只读）。当技能步骤过时、发现更好的做法时调用"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "技能名（不可改，用于定位）"},
                    "description": {"type": "string", "description": "新描述（一句话）"},
                    "content": {"type": "string", "description": "新正文"}
                },
                "required": ["name", "content"]
            }),
        },
        ToolDef {
            name: "delete_skill",
            description: "删除一个已过时、错误、不再需要的技能（连带其目录）。谨慎使用"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "技能名"}
                },
                "required": ["name"]
            }),
        },
        ToolDef {
            name: "install_skill",
            description: "按链接拉取并安装一个技能。用户给出 GitHub 等渠道的技能链接时调用。\
                          支持两种链接：单文件（github.com/.../blob/... 或 raw 直链）与\
                          目录（github.com/.../tree/<branch>/<path>，会拉取整个技能的 SKILL.md 与 references/ 参考文档）。\
                          也可传 name 显式指定技能名，description 可显式提供（缺省取 frontmatter）"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "指向技能 SKILL.md 的链接，或 github.com/.../tree/... 技能目录链接（http/https）"},
                    "name": {"type": "string", "description": "技能名（可选，缺省依次取 frontmatter name / 文件名 / 目录名）"},
                    "description": {"type": "string", "description": "技能描述（可选，缺省取 frontmatter）"}
                },
                "required": ["url"]
            }),
        },
    ]
}
