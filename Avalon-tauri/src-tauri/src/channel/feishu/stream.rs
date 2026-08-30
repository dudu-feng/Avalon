// 飞书输出：双通道
//
// 对齐 Python 版的交互形态：
//   正文 → 独立消息，每轮一条，可被引用、转发、在会话列表里有预览
//   过程 → 流式卡片（思考 + 工具调用），跑完折叠成 collapsible_panel 默认收起
//
// EngineEvent 由同步闭包发出（on_event 是 FnMut，里面不能 await），
// 所以全部 HTTP 都挪进本模块的 pump 任务，闭包只负责往 mpsc 里塞。
//
// 过程卡片是 lazy 创建的：纯聊天（无思考、无工具）根本不会产生卡片，
// 不然每句「你好」后面都跟一张空卡片。

use std::time::Duration;

use anyhow::Result;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::{interval_at, Instant, MissedTickBehavior};

use super::api::FeishuApi;
use crate::engine::EngineEvent;

/// 过程卡片里那个 markdown 元素的 id，流式更新都打在它身上。
/// 由我们自己指定 —— CardKit 不会自动生成，不写就没法定位元素
const ELEMENT_ID: &str = "stream_md";
/// 节流间隔。飞书卡片更新限 10 QPS（100ms），留出安全边际
const TICK: Duration = Duration::from_millis(120);
/// 过程卡片累积内容上限，超长会被飞书拒收
const MAX_CHARS: usize = 8000;
/// 单个代码块的长度上限（工具参数、工具结果都可能很长）
const CODE_LIMIT: usize = 4000;
/// 工具结果短于这个长度就直接跟在同一行，否则折进代码块
const INLINE_RESULT_LIMIT: usize = 200;
/// 卡片刚建出来时的占位。CardKit 不接受空内容
const PLACEHOLDER: &str = "思考中…";
/// 连续推送失败多少次后放弃流式，免得网络或权限问题导致刷屏
const MAX_FAILURES: u32 = 3;

/// 过程卡片里的一步
enum Step {
    /// 思考。ThoughtDelta 是流式增量，累积进同一段
    Thought(String),
    ToolCall { name: String, arguments: Value },
    ToolResult {
        name: String,
        success: bool,
        result: String,
    },
    Error(String),
}

/// 把过程事件累积成一份可渲染的 markdown
pub struct ProcessRenderer {
    steps: Vec<Step>,
}

impl ProcessRenderer {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    /// 思考是流式来的，续写末尾那段；中间隔了工具调用就另起一段
    pub fn push_thought(&mut self, delta: &str) {
        match self.steps.last_mut() {
            Some(Step::Thought(t)) => t.push_str(delta),
            _ => self.steps.push(Step::Thought(delta.to_string())),
        }
    }

    pub fn push_tool_call(&mut self, name: String, arguments: Value) {
        self.steps.push(Step::ToolCall { name, arguments });
    }

    pub fn push_tool_result(&mut self, name: String, success: bool, result: String) {
        self.steps.push(Step::ToolResult {
            name,
            success,
            result,
        });
    }

    pub fn push_error(&mut self, message: &str) {
        self.steps.push(Step::Error(message.to_string()));
    }

    /// 是否什么过程都没有 —— 纯聊天时据此跳过整张卡片
    pub fn is_empty(&self) -> bool {
        !self.steps.iter().any(|s| !render_step(s).is_empty())
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for step in &self.steps {
            let piece = render_step(step);
            if piece.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&piece);
        }
        if out.is_empty() {
            return PLACEHOLDER.to_string();
        }
        truncate(out, MAX_CHARS, "\n\n…（内容过长，已截断）")
    }
}

fn render_step(step: &Step) -> String {
    match step {
        Step::Thought(t) => {
            let t = t.trim();
            if t.is_empty() {
                String::new()
            } else {
                format!("💭 思考\n\n{t}")
            }
        }
        Step::ToolCall { name, arguments } => {
            let mut s = format!("🛠️ 调用工具：**{name}**");
            // 无参工具（比如 get_time）就别挂一个空的 {} 代码块了
            let has_args = match arguments {
                Value::Null => false,
                Value::Object(m) => !m.is_empty(),
                _ => true,
            };
            if has_args {
                let pretty = serde_json::to_string_pretty(arguments)
                    .unwrap_or_else(|_| arguments.to_string());
                s.push('\n');
                s.push_str(&code_block(&pretty));
            }
            s
        }
        Step::ToolResult {
            name,
            success,
            result,
        } => {
            let status = if *success { "✅" } else { "❌" };
            if result.chars().count() <= INLINE_RESULT_LIMIT {
                format!("{status} **{name}**：{result}")
            } else {
                format!("{status} **{name}** 结果：\n{}", code_block(result))
            }
        }
        Step::Error(m) => format!("⚠️ {m}"),
    }
}

/// 把文本安全包进代码块：内部的 ``` 会提前闭合代码块，必须替换掉
fn code_block(text: &str) -> String {
    let safe = truncate(
        text.replace("```", "'''"),
        CODE_LIMIT,
        "\n…（已截断）",
    );
    format!("```\n{safe}\n```")
}

/// 按字符（而非字节）截断，避免把多字节字符切碎
fn truncate(s: String, max: usize, suffix: &str) -> String {
    if s.chars().count() <= max {
        return s;
    }
    let kept: String = s.chars().take(max).collect();
    format!("{kept}{suffix}")
}

/// 流式过程卡片。lazy 创建 —— 第一次真有内容要推时才建
pub struct ProcessCard {
    api: FeishuApi,
    chat_id: String,
    open: Option<OpenCard>,
    failures: u32,
}

struct OpenCard {
    card_id: String,
    /// 必须严格递增，飞书据此丢弃乱序到达的旧内容
    sequence: u32,
    /// 上次成功推送的内容，相同则跳过
    last: String,
}

impl ProcessCard {
    pub fn new(api: FeishuApi, chat_id: String) -> Self {
        Self {
            api,
            chat_id,
            open: None,
            failures: 0,
        }
    }

    /// 推一次累积全文。首次调用时才真正创建并发出卡片
    async fn push(&mut self, content: &str) -> Result<()> {
        if self.failures >= MAX_FAILURES {
            anyhow::bail!("过程卡片连续 {MAX_FAILURES} 次更新失败，已放弃流式推送");
        }

        if self.open.is_none() {
            match self.create().await {
                Ok(card) => self.open = Some(card),
                Err(e) => {
                    self.failures += 1;
                    return Err(e);
                }
            }
        }

        let card = self.open.as_mut().expect("上面刚创建过");
        if content == card.last {
            return Ok(());
        }

        card.sequence += 1;
        match self
            .api
            .update_card_element(&card.card_id, ELEMENT_ID, content, card.sequence)
            .await
        {
            Ok(()) => {
                card.last = content.to_string();
                self.failures = 0;
                Ok(())
            }
            Err(e) => {
                self.failures += 1;
                Err(e)
            }
        }
    }

    async fn create(&self) -> Result<OpenCard> {
        let card_id = self
            .api
            .create_card(&json!({
                "schema": "2.0",
                "config": {
                    // 打开流式态，客户端才有逐字浮现的动画
                    "streaming_mode": true,
                    "summary": { "content": "思考过程" },
                },
                "body": {
                    "elements": [{
                        "tag": "markdown",
                        "element_id": ELEMENT_ID,
                        "content": PLACEHOLDER,
                    }]
                }
            }))
            .await?;

        self.api.send_card(&self.chat_id, &card_id).await?;

        Ok(OpenCard {
            card_id,
            sequence: 0,
            last: PLACEHOLDER.to_string(),
        })
    }

    /// 收尾：推最终全文 → 关流式 → 重构成默认收起的折叠面板。
    ///
    /// 卡片没建起来（纯聊天，或建卡片就失败了）时什么都不做。
    pub async fn finish(mut self, content: &str) -> Result<()> {
        if self.open.is_none() {
            return Ok(());
        }

        // 推最终内容。失败也要继续往下关流式，否则卡片永远转圈
        let push_err = self.push(content).await.err();

        let card = self.open.as_mut().expect("上面判过非空");

        card.sequence += 1;
        let finish_err = self
            .api
            .finish_card_streaming(&card.card_id, card.sequence)
            .await
            .err();

        // 折叠：把整段过程收进 collapsible_panel，默认不展开，
        // 免得几十行思考把正文挤到看不见的地方
        card.sequence += 1;
        let fold_err = self
            .api
            .update_card(&card.card_id, &folded_card(content), card.sequence)
            .await
            .err();

        match push_err.or(finish_err).or(fold_err) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// 折叠面板形态的卡片快照。collapsible_panel 是卡片 JSON 2.0 的容器组件
fn folded_card(content: &str) -> Value {
    json!({
        "schema": "2.0",
        // 显式关掉流式态：全量替换会覆盖 config，不写的话行为不确定
        "config": {
            "streaming_mode": false,
            "summary": { "content": "思考过程" },
        },
        "body": {
            "elements": [{
                "tag": "collapsible_panel",
                "expanded": false,
                "header": { "title": { "tag": "plain_text", "content": "💭 思考过程" } },
                "elements": [{ "tag": "markdown", "content": content }],
            }]
        }
    })
}

/// pump 跑完后交还给调用方的东西
pub struct PumpResult {
    pub card: ProcessCard,
    pub renderer: ProcessRenderer,
    /// 是否发出过正文消息。一条都没发时调用方要兜底回一句
    pub sent_any: bool,
}

/// 消费 EngineEvent：正文攒够一轮就发消息，过程节流推卡片。
///
/// 不负责收尾 —— Engine 的 Err 只有调用方拿得到，要等它追加错误后再 finish。
pub async fn pump(
    api: FeishuApi,
    chat_id: String,
    mut rx: mpsc::UnboundedReceiver<EngineEvent>,
) -> PumpResult {
    let mut renderer = ProcessRenderer::new();
    let mut card = ProcessCard::new(api.clone(), chat_id.clone());
    let mut message = String::new();
    let mut sent_any = false;
    let mut dirty = false;

    // interval 的首个 tick 会立即触发，用 interval_at 推迟一整个周期，
    // 免得卡片刚建好就被推一次和占位符相同的内容
    let mut ticker = interval_at(Instant::now() + TICK, TICK);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Some(ev) => {
                    // 调工具前、新一轮开始前，先把攒着的正文发出去 ——
                    // 「我先查一下」要出现在工具调用卡片之前，顺序才符合直觉
                    if matches!(ev, EngineEvent::ToolCall { .. } | EngineEvent::RoundStart)
                        && flush(&api, &chat_id, &mut message).await
                    {
                        sent_any = true;
                    }

                    match ev {
                        EngineEvent::MessageDelta { delta } => message.push_str(&delta),
                        EngineEvent::ThoughtDelta { delta } => {
                            renderer.push_thought(&delta);
                            dirty = true;
                        }
                        EngineEvent::ToolCall { tool_name, arguments, .. } => {
                            renderer.push_tool_call(tool_name, arguments);
                            dirty = true;
                        }
                        EngineEvent::ToolResult { tool_name, success, result } => {
                            renderer.push_tool_result(tool_name, success, result);
                            dirty = true;
                        }
                        EngineEvent::Error { message: m, .. } => {
                            renderer.push_error(&m);
                            dirty = true;
                        }
                        EngineEvent::Done { result } => {
                            // 兜底：模型不走流式增量、只在结束时一次性给全文的情况
                            if message.trim().is_empty() {
                                message = result.message;
                            }
                        }
                        EngineEvent::RoundStart => {}
                    }
                }
                // 发送端随 Engine::run 的闭包一起 drop，这是正常的结束信号
                None => break,
            },
            _ = ticker.tick(), if dirty => {
                dirty = false;
                if let Err(e) = card.push(&renderer.render()).await {
                    log::warn!(target: "feishu", "过程卡片更新失败: {e:#}");
                }
            }
        }
    }

    // 最后一轮的正文
    if flush(&api, &chat_id, &mut message).await {
        sent_any = true;
    }

    PumpResult {
        card,
        renderer,
        sent_any,
    }
}

/// 把攒着的正文作为一条独立消息发出去，返回是否真的发了
async fn flush(api: &FeishuApi, chat_id: &str, buf: &mut String) -> bool {
    let text = buf.trim().to_string();
    buf.clear();
    if text.is_empty() {
        return false;
    }
    if let Err(e) = api.send_markdown(chat_id, &text).await {
        log::error!(target: "feishu", "正文消息发送失败: {e:#}");
        return false;
    }
    true
}
