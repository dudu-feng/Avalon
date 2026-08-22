// TaskStore：定时任务的持久化存储（内存累计 + 原子写，对齐 UsageStore 模式）
//
// 被三方共享：Scheduler（读到期任务）、ToolSet（agent 经 tool 增删查）、commands（用户 UI 增删查）。
// 护栏集中在 create：prompt 长度 + agent 数量上限（source=Agent 时）。

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};

use super::{RunStatus, ScheduleType, ScheduledTask, TaskSource, TaskRunMeta};

/// agent 创建的任务数量上限（护栏）
pub const AGENT_TASK_LIMIT: usize = 10;
/// 任务内容最大长度（字符）
pub const PROMPT_MAX_CHARS: usize = 500;
/// 任务名称最大长度（字符）
pub const NAME_MAX_CHARS: usize = 50;

pub struct TaskStore {
    path: PathBuf,
    inner: Mutex<Vec<ScheduledTask>>,
}

impl TaskStore {
    /// 构造：读一次文件进内存；文件不存在/损坏 → 空列表静默降级（不影响启动）
    pub fn new(path: PathBuf) -> Self {
        let tasks = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<ScheduledTask>>(&s).ok())
            .unwrap_or_default();
        Self {
            path,
            inner: Mutex::new(tasks),
        }
    }

    /// 全部任务（创建时间倒序，最新在前）
    pub fn list(&self) -> Vec<ScheduledTask> {
        let mut tasks = self.inner.lock().unwrap().clone();
        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        tasks
    }

    /// 创建任务（护栏：name/prompt 非空/长度、agent 数量上限）
    pub fn create(
        &self,
        source: TaskSource,
        name: &str,
        prompt: &str,
        schedule: ScheduleType,
    ) -> Result<ScheduledTask> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("任务名称不能为空"));
        }
        if name.chars().count() > NAME_MAX_CHARS {
            return Err(anyhow!("任务名称过长（上限 {} 字符）", NAME_MAX_CHARS));
        }
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(anyhow!("任务内容不能为空"));
        }
        if prompt.chars().count() > PROMPT_MAX_CHARS {
            return Err(anyhow!("任务内容过长（上限 {} 字符）", PROMPT_MAX_CHARS));
        }

        let mut tasks = self.inner.lock().unwrap();
        if source == TaskSource::Agent {
            let count = tasks.iter().filter(|t| t.source == TaskSource::Agent).count();
            if count >= AGENT_TASK_LIMIT {
                return Err(anyhow!(
                    "智能体创建的任务已达上限（{}），请先删除部分",
                    AGENT_TASK_LIMIT
                ));
            }
        }

        let now = chrono::Local::now();
        let task = ScheduledTask {
            id: format!("task_{}", now.timestamp_millis()),
            source,
            name: name.to_string(),
            prompt: prompt.to_string(),
            schedule,
            enabled: true,
            created_at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            last_run_at: None,
            runs: Vec::new(),
        };
        tasks.push(task.clone());
        self.write_locked(&tasks[..])?;
        Ok(task)
    }

    /// 删除任务
    pub fn delete(&self, id: &str) -> Result<()> {
        let mut tasks = self.inner.lock().unwrap();
        let before = tasks.len();
        tasks.retain(|t| t.id != id);
        if tasks.len() == before {
            return Err(anyhow!("任务 '{id}' 不存在"));
        }
        self.write_locked(&tasks[..])
    }

    /// 停用 / 启用
    pub fn toggle(&self, id: &str, enabled: bool) -> Result<()> {
        let mut tasks = self.inner.lock().unwrap();
        let task = tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| anyhow!("任务 '{id}' 不存在"))?;
        task.enabled = enabled;
        self.write_locked(&tasks[..])
    }

    /// 到期且未执行的任务列表（心跳循环消费）
    pub fn due_tasks(&self) -> Vec<ScheduledTask> {
        let now = chrono::Local::now();
        self.inner
            .lock()
            .unwrap()
            .iter()
            .filter(|t| super::is_due(t, now))
            .cloned()
            .collect()
    }

    /// 记录一次执行结果：追加 run 元数据 + 更新 last_run_at，写回
    pub fn mark_ran(&self, id: &str, status: RunStatus) -> Result<()> {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let mut tasks = self.inner.lock().unwrap();
        let task = tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| anyhow!("任务 '{id}' 不存在"))?;
        task.last_run_at = Some(ts.clone());
        task.runs.push(TaskRunMeta {
            triggered_at: ts,
            status,
            read: false,
        });
        self.write_locked(&tasks[..])
    }

    /// 清空某任务的未读标记（前端查看执行历史后调用）
    pub fn mark_read(&self, id: &str) -> Result<()> {
        let mut tasks = self.inner.lock().unwrap();
        let task = tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| anyhow!("任务 '{id}' 不存在"))?;
        for r in &mut task.runs {
            r.read = true;
        }
        self.write_locked(&tasks[..])
    }

    /// 未读执行总数（驱动侧边栏角标）
    pub fn unread_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .flat_map(|t| &t.runs)
            .filter(|r| !r.read)
            .count()
    }

    /// 锁内序列化 + 原子写（tmp + rename，自动建目录；与 usage store 同款）
    fn write_locked(&self, tasks: &[ScheduledTask]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建定时任务目录失败: {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(tasks).context("序列化定时任务失败")?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &content)
            .with_context(|| format!("写入定时任务临时文件失败: {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("原子替换定时任务文件失败: {}", self.path.display()))?;
        Ok(())
    }
}
