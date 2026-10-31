// 文件存储模块
//
// 统一管理 data/file 下的落盘规则：日期归档、命名、内容嗅探、哈希、
// 原子写、sidecar 元数据、路径越界校验与过期清理。
// 目录布局见 doc/v0.7/02-file-storage.md。

mod store;
mod types;

pub use store::{
    FileStore, SaveRequest, MAX_FILE_BYTES, QUARANTINE_TTL_DAYS, TEMP_TTL_HOURS,
};
// Sidecar / ImageMeta / DerivedPaths 等由测试与后续缓存、飞书接入使用
#[allow(unused_imports)]
pub use types::{
    CacheKind, CleanupReport, DerivedPaths, FileKind, FileSource, ImageMeta, Sidecar, StoredFile,
    SIDECAR_SCHEMA,
};
