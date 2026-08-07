// 配置中心模块
//
// 管理 Avalon 应用的所有配置项，包括 LLM API、路径、向量数据库、会话记忆等。
// 配置文件为 JSON 格式，存储在用户数据目录下。
//
// 使用方式：
//     use config::AppConfig;
//     let config = AppConfig::load()?;
//     let api_key = config.llm.api_key;

pub mod app_config;

pub use app_config::AppConfig;
