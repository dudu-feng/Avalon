// 联网搜索工具 schema。

use serde_json::json;

use super::ToolDef;

/// 联网搜索工具定义（仅注入 SearchClient 时暴露，2 个）
pub(crate) fn web_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "web_search",
            // 明确分工：这里只给摘要，要正文得再调 read_web_page。
            // 不写清楚的话模型容易拿着一句摘要就下结论
            description: "搜索互联网获取实时信息。返回若干条结果的标题、链接与摘要；\
                          需要某条结果的完整内容时，再用 read_web_page 打开它的链接"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "搜索关键词"},
                    "max_results": {"type": "integer", "description": "返回条数（可选，1-10，缺省取配置值）"}
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "read_web_page",
            description: "读取指定网页的正文并转为 Markdown。只支持 http/https 网页，\
                         PDF、图片、音视频等二进制格式无法读取。正文过长会被截断，\
                         截断时会在结尾提示下一个 offset，带上 offset 参数继续读下一段"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "网页链接，必须是 http 或 https"},
                    "offset": {"type": "integer", "description": "续读起始字符位置（可选，默认 0 从头读）"}
                },
                "required": ["url"]
            }),
        },
    ]
}
