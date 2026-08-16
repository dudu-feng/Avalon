// 工具注册表：工具元数据集中管理 + 按名分发
//
// get_tool_list 按注册顺序拼格式化列表（供 LLM 理解可用工具）；
// invoke_tool 用 match 分发（5 个工具 O(1) 命中，替代 Python 线性遍历）。

#![allow(dead_code)] // tool 模块供未来 engine 引用，当前无调用方，接入后移除

use async_trait::async_trait;
use serde_json::Value;

use super::fs_tools;
use super::ToolRegistry;

/// 工具元数据（名字 + 描述，供 get_tool_list 拼给 LLM）
struct ToolDef {
    name: &'static str,
    description: &'static str,
}

const TOOL_DEFS: &[ToolDef] = &[
    ToolDef {
        name: "read_file",
        description: "读取指定文件内容。参数 file_path: 文件路径（字符串）",
    },
    ToolDef {
        name: "write_file",
        description: "创建或覆盖写入文件。参数 file_path: 文件路径（字符串）, content: 文件内容（字符串）",
    },
    ToolDef {
        name: "delete_file",
        description: "删除指定文件。参数 file_path: 文件路径（字符串）",
    },
    ToolDef {
        name: "run_shell_command",
        description: "在终端执行命令并返回标准输出与错误。参数 command: 命令字符串",
    },
    ToolDef {
        name: "get_directory_contents",
        description: "获取目录下的文件和子目录。参数 directory_path: 目录路径（字符串）",
    },
];

/// 工具注册表实现（当前无外部依赖，未来注入 MemoryIndex 做记忆检索）
pub struct ToolSet;

impl ToolSet {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolRegistry for ToolSet {
    fn get_tool_list(&self) -> String {
        let mut out = String::from("## 可用工具列表\n");
        for t in TOOL_DEFS {
            out.push_str(&format!("- **{}**: {}\n", t.name, t.description));
        }
        out.trim_end().to_string()
    }

    async fn invoke_tool(&self, name: &str, args: &Value) -> String {
        match name {
            "read_file" => fs_tools::read_file(args),
            "write_file" => fs_tools::write_file(args),
            "delete_file" => fs_tools::delete_file(args),
            "run_shell_command" => fs_tools::run_shell_command(args).await,
            "get_directory_contents" => fs_tools::get_directory_contents(args),
            _ => format!("未找到工具: {name}"),
        }
    }
}
