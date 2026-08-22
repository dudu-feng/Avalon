// Scheduler：心跳循环（L3 静默层调度 + L2 静默执行）
//
// 每 tick 检查到期任务，逐个 spawn 静默 run（on_event 空闭包，产出落 session，不推前端），
// 完成后记录执行元数据 + 发全局事件通知前端（未读角标）。
// 并发安全：同一任务上一次执行未结束则跳过（skip if running）；任务 channel 与用户 channel 文件隔离。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::engine::Engine;

use super::store::TaskStore;
use super::RunStatus;

pub struct Scheduler {
    engine: Arc<Engine>,
    store: Arc<TaskStore>,
    app: AppHandle,
}

impl Scheduler {
    pub fn new(engine: Arc<Engine>, store: Arc<TaskStore>, app: AppHandle) -> Self {
        Self { engine, store, app }
    }

    /// 心跳循环：每 tick 检查到期任务，静默执行 + 全局事件通知
    pub async fn run_loop(&self, tick: Duration) {
        let mut interval = tokio::time::interval(tick);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip); // 错过跳过（D11）
        loop {
            interval.tick().await;
            for task in self.store.due_tasks() {
                // 同一任务上一次执行未结束则跳过，避免并发执行同一 channel
                if self.engine.is_channel_busy(task.channel()) {
                    continue;
                }
                let engine = self.engine.clone();
                let store = self.store.clone();
                let app = self.app.clone();
                tokio::spawn(async move {
                    let channel = task.channel().to_string();
                    // 执行前 ensure_active + 写入可读标题（id 不变，title = 定时任务-任务名称）
                    let _ = engine.set_current_title(&channel, &task.session_title());
                    let cancel = Arc::new(AtomicBool::new(false));
                    // 静默：on_event 空闭包，执行过程落 session，不推前端
                    let result = engine.run(&task.prompt, &channel, cancel, |_ev| {}).await;
                    let status = if result.is_ok() {
                        RunStatus::Succeeded
                    } else {
                        RunStatus::Failed
                    };
                    let _ = store.mark_ran(&task.id, status);
                    let _ = app.emit("task-finished", task.id.clone());
                });
            }
        }
    }
}
