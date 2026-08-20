// 运行时配置存储
//
// 用 RwLock 共享当前配置快照；保存时先写盘再更新内存，
// 依赖方（后续的 LLM / 向量等模块）随时 get() 读取最新值。

use std::sync::{Arc, RwLock};

use anyhow::Result;

use super::loader;
use super::types::AppConfig;

/// 可克隆的配置存储句柄，跨 Tauri State 与各模块共享
#[derive(Clone)]
pub struct ConfigStore {
    inner: Arc<RwLock<AppConfig>>,
}

impl ConfigStore {
    /// 从磁盘加载配置
    pub fn load() -> Result<Self> {
        let config = loader::load()?;
        Ok(Self {
            inner: Arc::new(RwLock::new(config)),
        })
    }

    /// 用内存配置构造（加载失败时的兜底）
    pub fn from_config(config: AppConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(config)),
        }
    }

    /// 只读快照
    pub fn get(&self) -> AppConfig {
        self.inner.read().expect("config lock poisoned").clone()
    }

    /// 保存：写回磁盘 + 更新内存，返回校验警告列表
    pub fn save(&self, next: AppConfig) -> Result<Vec<String>> {
        let warnings = loader::validate(&next);
        loader::save(&next)?;
        *self.inner.write().expect("config lock poisoned") = next;
        Ok(warnings)
    }

    /// 切换活跃模型：校验 name 存在，改 active_model 后复用 save 写回
    pub fn set_active_model(&self, name: &str) -> Result<Vec<String>> {
        let mut next = self.get();
        if !next.models.iter().any(|m| m.name == name) {
            return Err(anyhow::anyhow!("模型 '{name}' 不存在"));
        }
        next.active_model = name.to_string();
        self.save(next)
    }

    /// 校验当前配置
    pub fn validate(&self) -> Vec<String> {
        loader::validate(&self.get())
    }
}
