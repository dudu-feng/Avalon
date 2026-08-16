// 配置中心模块
//
// 职责：
//   - 加载 data/config.toml（loader）
//   - 定位配置文件 + 派生共享 data 路径（paths）
//   - 配置数据结构（types）
//   - 运行时共享与保存（store）

pub mod loader;
pub mod paths;
pub mod store;
pub mod types;

pub use loader::default_config;
pub use store::ConfigStore;
pub use types::*;
