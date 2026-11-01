// QQ 官方机器人后端
//
// token 获取与缓存、REST、网关、输出适配、消息处理各自独立，
// 由上层 qq::run 负责拼装。

pub mod api;
pub mod gateway;
pub mod handler;
pub mod out;
pub mod proto;
pub mod token;

pub use api::QqApi;
pub use gateway::test_credentials;
pub use handler::MessageHandler;
pub use token::TokenProvider;
