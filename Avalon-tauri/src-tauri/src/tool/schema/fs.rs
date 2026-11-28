// 基础文件 / 终端工具 schema。

use serde_json::json;

use super::ToolDef;
use super::super::sandbox::Sandbox;

/// 基础文件/终端工具定义（5 个，固定注册）。
/// 描述随沙箱配置变化 —— 用户改了工作区，模型下一轮看到的就是新边界
pub(crate) fn tool_defs(sb: &Sandbox) -> Vec<ToolDef> {
    let scope = format!("只能访问工作区内的路径，当前工作区：{}", sb.roots_hint());
    vec![
        ToolDef {
            name: "read_file",
            description: format!("读取指定文件内容。{scope}"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "文件路径"}
                },
                "required": ["file_path"]
            }),
        },
        ToolDef {
            name: "write_file",
            description: format!("创建或覆盖写入文件，父目录会自动创建。{scope}"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "文件路径"},
                    "content": {"type": "string", "description": "文件内容"}
                },
                "required": ["file_path", "content"]
            }),
        },
        ToolDef {
            name: "delete_file",
            description: format!("删除指定文件。{scope}"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "文件路径"}
                },
                "required": ["file_path"]
            }),
        },
        ToolDef {
            name: "run_shell_command",
            // 「不经过 shell」必须写进描述里。否则模型会按惯例拼 `a && b`、
            // 用 > 重定向，然后对着一个「参数被当成字面量」的怪结果反复试
            description: format!(
                "执行一条系统命令并返回输出。命令不经过 shell 解释：参数必须逐个放进 args 数组，\
                 && || | > < ; 等符号不起作用，无法使用管道、重定向和命令拼接。\
                 需要读写文件请用 read_file / write_file。当前允许的命令：{}",
                sb.allowlist_hint()
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "命令名本身，不带路径、不带参数，例如 ping"},
                    "args": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "参数列表，每个参数一项，例如 [\"-n\", \"1\", \"127.0.0.1\"]"
                    }
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "get_directory_contents",
            description: format!("获取目录下的文件和子目录。{scope}"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "directory_path": {"type": "string", "description": "目录路径"}
                },
                "required": ["directory_path"]
            }),
        },
    ]
}
