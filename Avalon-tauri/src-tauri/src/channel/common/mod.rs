// 渠道通用原语
//
// 第二个渠道（QQ）出现后，飞书里已经验证过的三件并发/数据工具才值得抽出来：
// 事件去重、每会话串行锁、图片先确认后合并。它们只依赖 std/tokio，与具体协议无关。
//
// 刻意不抽「完整 ChannelAdapter trait」：连接、事件格式、发送 API 在两个渠道之间
// 差异远大于共性，过早抽象只会逼着两边互相迁就。等第三个渠道再评估。

pub mod dedup;
pub mod key;
pub mod pending;
pub mod slot;

pub use dedup::Dedup;
pub use key::sanitize_channel;
pub use pending::PendingImages;
pub use slot::{ChannelSlot, SlotRegistry, MAX_WAITING};
