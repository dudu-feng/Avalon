// Scheduler：心跳循环（L3 静默层调度 + L2 静默执行）
//
// 每 tick 检查到期任务，逐个 spawn 一次任务 run（作为 source=task 的 user 消息写入 main），
// 完成后记录执行元数据 + 发全局事件通知前端（未读角标），并按 notify_channel 投递结果。
// 并发安全：全局队列有正在执行 / 排队中的消息时跳过本次触发（skip if busy）。

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::engine::{Engine, EngineEvent, ReplyTarget, RunContext, UserInput};
use crate::session::MessageOrigin;

use super::store::TaskStore;
use super::{RunStatus, TaskNotifier};

pub struct Scheduler {
    engine: Arc<Engine>,
    store: Arc<TaskStore>,
    app: AppHandle,
    notifier: Arc<dyn TaskNotifier>,
}

impl Scheduler {
    pub fn new(
        engine: Arc<Engine>,
        store: Arc<TaskStore>,
        app: AppHandle,
        notifier: Arc<dyn TaskNotifier>,
    ) -> Self {
        Self {
            engine,
            store,
            app,
            notifier,
        }
    }

    /// 心跳循环：每 tick 检查到期任务，静默执行 + 全局事件通知
    pub async fn run_loop(&self, tick: Duration) {
        let mut interval = tokio::time::interval(tick);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip); // 错过跳过（D11）
        loop {
            interval.tick().await;
            for task in self.store.due_tasks() {
                // 统一会话只有一个写者：任一来源正在执行 / 排队时，任务本次直接跳过，
                // 不积压。30 秒后下一 tick 仍到期，会再尝试
                if self.engine.pending_count() > 0 {
                    log::debug!(target: "scheduler", "消息队列忙碌，跳过任务本次触发 {}", task.id);
                    continue;
                }
                let engine = self.engine.clone();
                let store = self.store.clone();
                let app = self.app.clone();
                let notifier = self.notifier.clone();
                tokio::spawn(async move {
                    log::info!(target: "scheduler", "触发任务 {} ({})", task.id, task.name);

                    if let Err(e) = engine.init_session() {
                        log::warn!(target: "scheduler", "初始化主会话失败 {}: {e:#}", task.id);
                    }

                    // 任务不自动回复触发渠道；需要通知时由 notify_channel 在 run 后投递
                    let origin = MessageOrigin::task_owner(
                        Some(task.id.clone()),
                        Some(task.name.clone()),
                    );
                    let run = RunContext::new(
                        Engine::new_run_id("task"),
                        origin.clone(),
                        ReplyTarget::None,
                    );
                    let input = UserInput {
                        for_model: &task.prompt,
                        for_history: &task.prompt,
                        meta: Some(origin),
                        attachments: Vec::new(),
                    };

                    // 静默：过程事件不推前端，仅收集最终回复用于通知
                    let mut final_text = String::new();
                    let result = engine
                        .run_with_input(input, run, |ev| {
                            if let EngineEvent::Done { result } = ev {
                                final_text = result.message;
                            }
                        })
                        .await;

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

                    // 通知正文只取最终回复，不发工具中间态；失败只记 Warn / notify_error
                    let notify_error = if result.is_ok()
                        && task.notify_channel != super::TaskNotifyChannel::None
                    {
                        let body = final_text.trim();
                        if body.is_empty() {
                            log::warn!(target: "scheduler", "任务 {} 没有最终回复，跳过通知", task.id);
                            None
                        } else {
                            match notifier
                                .notify(
                                    task.notify_channel,
                                    task.notify_target.as_deref(),
                                    task.notify_title(),
                                    body,
                                )
                                .await
                            {
                                Ok(()) => None,
                                Err(e) => {
                                    log::warn!(
                                        target: "scheduler",
                                        "任务 {} 结果通知发送失败: {e:#}", task.id
                                    );
                                    Some(format!("{e:#}"))
                                }
                            }
                        }
                    } else {
                        None
                    };

                    // 这条落盘失败会让 last_run 停在旧值，下一个 tick 判定仍到期 → 重复执行
                    if let Err(e) = store.mark_ran(&task.id, status, notify_error) {
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
