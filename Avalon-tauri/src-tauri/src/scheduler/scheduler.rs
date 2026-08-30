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
                // 同一任务上一次执行未结束则跳过，避免并发执行同一 channel。
                // 用 debug 不用 info —— 任务真卡住时这里 30 秒刷一条，会淹没其它日志
                if self.engine.is_channel_busy(task.channel()) {
                    log::debug!(target: "scheduler", "任务 {} 上一轮未结束，跳过本次", task.id);
                    continue;
                }
                let engine = self.engine.clone();
                let store = self.store.clone();
                let app = self.app.clone();
                tokio::spawn(async move {
                    let channel = task.channel().to_string();
                    log::info!(target: "scheduler", "触发任务 {} ({})", task.id, task.name);

                    // 执行前 ensure_active + 写入可读标题（id 不变，title = 定时任务-任务名称）
                    if let Err(e) = engine.set_current_title(&channel, &task.session_title()) {
                        log::warn!(target: "scheduler", "写入会话标题失败 {}: {e:#}", task.id);
                    }
                    let cancel = Arc::new(AtomicBool::new(false));
                    // 静默：on_event 空闭包，执行过程落 session，不推前端
                    let result = engine.run(&task.prompt, &channel, cancel, |_ev| {}).await;

                    // 失败原因必须落日志 —— 之前只把 Failed 记进 store，
                    // 用户看到红点却查不出为什么失败
                    let status = match &result {
                        Ok(()) => {
                            log::info!(target: "scheduler", "任务 {} 执行完成", task.id);
                            RunStatus::Succeeded
                        }
                        Err(e) => {
                            log::error!(target: "scheduler", "任务 {} 执行失败: {e:#}", task.id);
                            RunStatus::Failed
                        }
                    };

                    // 这条落盘失败会让 last_run 停在旧值，下一个 tick 判定仍到期 → 重复执行
                    if let Err(e) = store.mark_ran(&task.id, status) {
                        log::error!(
                            target: "scheduler",
                            "记录执行结果失败 {}，任务可能被重复触发: {e:#}", task.id
                        );
                    }
                    let _ = app.emit("task-finished", task.id.clone());
                });
            }
        }
    }
}
