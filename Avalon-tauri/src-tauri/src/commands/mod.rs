// Tauri IPC 命令定义（按域拆分）
//
// 前端通过 invoke("command_name", { params }) 调用；命令名由函数名决定，模块路径不影响 IPC。

pub mod config;
pub mod files;
pub mod vision;
pub mod chat;
pub mod session;
pub mod usage;
pub mod soul;
pub mod skill;
pub mod scheduler;
pub mod channel;
pub mod contacts;
