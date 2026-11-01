// 图片先确认、后合并
//
// 外部渠道的图片与文字通常分两条消息到达。收到图片就立刻调用视觉模型，等于在
// 用户还没提出问题时先跑一次多余的理解。这里把下载好的图片挂起，先回一段可配置的
// 询问文本，等下一条文字消息到达时再一起交给 Engine。
//
// 按「聊天」分组而不是按 session_mode 分组：统一模式下所有群/私聊共享会话，
// 但 A 私聊发的图不该被 B 在群里的一句文字带走。

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 已接收但还没等到文字说明的图片保留时长
pub const PENDING_IMAGE_TTL: Duration = Duration::from_secs(600);

struct PendingImage {
    path: String,
    received_at: Instant,
}

pub struct PendingImages {
    map: HashMap<String, Vec<PendingImage>>,
    ttl: Duration,
}

impl PendingImages {
    pub fn new(ttl: Duration) -> Self {
        Self {
            map: HashMap::new(),
            ttl,
        }
    }

    /// 存入一张图片，返回本次是否需要发送询问文本。
    /// 该会话此前没有待处理图片时为 true —— 连发多张只问一次。
    pub fn push(&mut self, channel: String, path: String, now: Instant) -> bool {
        // 顺手清理所有会话的过期项，避免长期运行内存缓慢增长
        let ttl = self.ttl;
        self.map.retain(|_, list| {
            list.retain(|p| now.duration_since(p.received_at) < ttl);
            !list.is_empty()
        });

        let list = self.map.entry(channel).or_default();
        let ask = list.is_empty();
        list.push(PendingImage {
            path,
            received_at: now,
        });
        ask
    }

    /// 取走某会话的图片；过期项直接丢弃
    pub fn take(&mut self, channel: &str, now: Instant) -> Vec<String> {
        let ttl = self.ttl;
        self.map
            .remove(channel)
            .unwrap_or_default()
            .into_iter()
            .filter(|p| now.duration_since(p.received_at) < ttl)
            .map(|p| p.path)
            .collect()
    }
}

impl Default for PendingImages {
    fn default() -> Self {
        Self::new(PENDING_IMAGE_TTL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_images_in_one_channel_ask_once() {
        let mut pending = PendingImages::default();
        let now = Instant::now();

        assert!(
            pending.push("chat-a".into(), "1.png".into(), now),
            "首张应询问"
        );
        assert!(
            !pending.push("chat-a".into(), "2.png".into(), now),
            "连发第二张不再询问"
        );
        assert!(
            pending.push("chat-b".into(), "3.png".into(), now),
            "不同会话各自询问"
        );

        assert_eq!(pending.take("chat-a", now), vec!["1.png", "2.png"]);
        assert!(pending.take("chat-a", now).is_empty(), "取走后应清空");
        assert_eq!(pending.take("chat-b", now), vec!["3.png"]);
    }

    #[test]
    fn expired_images_are_dropped_on_take() {
        let mut pending = PendingImages::default();
        let start = Instant::now();
        pending.push("chat-a".into(), "old.png".into(), start);

        let later = start + PENDING_IMAGE_TTL;
        assert!(pending.take("chat-a", later).is_empty());
    }

    #[test]
    fn push_prunes_expired_entries_of_other_channels() {
        let mut pending = PendingImages::default();
        let start = Instant::now();
        pending.push("chat-a".into(), "old.png".into(), start);

        let later = start + PENDING_IMAGE_TTL;
        assert!(pending.push("chat-b".into(), "new.png".into(), later));
        assert!(pending.take("chat-a", later).is_empty(), "过期项应被清理");
        assert_eq!(pending.take("chat-b", later), vec!["new.png"]);
    }
}
