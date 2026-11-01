// 渠道事件去重
//
// 两个渠道的服务端都可能重投同一条事件：网络抖动、ACK 丢失、客户端重连补发。
// 没有去重就会把同一句话跑两遍 ReAct，既浪费 token，也可能产生重复副作用
// （写文件、发通知）。
//
// 用事件 id 做 key，TTL + 容量双上限控制内存。TTL 过期与容量兜底是两条独立的
// 清理路径：短时间大量事件时 TTL 可能都还没到，容量兜底防止内存无限增长。

use std::collections::HashMap;
use std::time::{Duration, Instant};

pub struct Dedup {
    seen: HashMap<String, Instant>,
    ttl: Duration,
    capacity: usize,
}

impl Dedup {
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            seen: HashMap::new(),
            ttl,
            capacity: capacity.max(1),
        }
    }

    /// 首次出现返回 true；重复返回 false。
    pub fn accept(&mut self, id: &str) -> bool {
        let now = Instant::now();
        self.seen.retain(|_, t| now.duration_since(*t) < self.ttl);

        if self.seen.len() >= self.capacity {
            // TTL 没来得及清理时的兜底：按时间排序砍掉较旧的一半
            let mut entries: Vec<_> = self.seen.iter().map(|(k, t)| (k.clone(), *t)).collect();
            entries.sort_by_key(|(_, t)| *t);
            for (k, _) in entries.into_iter().take(self.capacity / 2) {
                self.seen.remove(&k);
            }
        }

        self.seen.insert(id.to_string(), now).is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_id_is_rejected_once() {
        let mut d = Dedup::new(Duration::from_secs(60), 16);
        assert!(d.accept("e1"));
        assert!(!d.accept("e1"));
        assert!(d.accept("e2"));
    }

    #[test]
    fn expired_id_can_be_accepted_again() {
        let mut d = Dedup::new(Duration::from_millis(1), 16);
        assert!(d.accept("e1"));
        std::thread::sleep(Duration::from_millis(3));
        assert!(d.accept("e1"));
    }

    #[test]
    fn capacity_evicts_old_entries() {
        let mut d = Dedup::new(Duration::from_secs(60), 4);
        for i in 0..4 {
            assert!(d.accept(&format!("e{i}")));
        }
        assert!(d.accept("new"), "容量满时应先腾出空间");
        assert!(d.seen.len() <= 4);
    }
}
