// ReAct 单循环编排
//
// 单模型 ReAct：一轮 chat_stream（带 tools）产出正文 + 思考 + tool_calls，
// 有 tool_calls 则逐个执行工具、以 role=tool 回填、继续循环，直到无工具调用。
// 全部中间态经 on_event 发射，收尾做会话持久化。
// 依赖 trait（ToolRegistry / SessionStore），llm 通过 LlmState 动态构建 client（配置热更新）。

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use serde_json::{json, Value};

use crate::config::ConfigStore;
use crate::llm::{ChatResult, LlmState, StreamEvent, TokenUsage};
use crate::prompt::PromptAssembler;
use crate::session::{Message, SessionStore};
use crate::tool::ToolRegistry;
use crate::usage::UsageStore;

use super::events::EngineEvent;
use super::history;

/// ReAct 循环核心（依赖 trait object，可注入 mock 单测循环逻辑）
pub(crate) async fn run_loop<F>(
    user_input: &str,
    channel: &str,
    config: &ConfigStore,
    llm: &LlmState,
    prompt: &PromptAssembler,
    tools: &dyn ToolRegistry,
    session: &dyn SessionStore,
    usage: &UsageStore,
    cancel: &AtomicBool,
    on_event: &mut F,
) -> Result<()>
where
    F: FnMut(EngineEvent) + Send,
{
    let tools_schema = tools.get_tools_schema();
    let session_context = session.get_context_for_prompt(channel)?;
    let system_prompt = prompt.assemble_chat_prompt(&session_context)?;
    let cfg = config.get();
    let model = cfg
        .active_model_config()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("未配置活跃模型（active_model 无效）"))?;
    let model_name = model.modelname.clone();
    let client = llm.client(model, cfg.llm.clone());

    // 本轮 messages：system + user + 循环内累加的 assistant(tool_calls)/tool 消息
    let mut messages: Vec<Value> = vec![
        json!({"role": "system", "content": system_prompt}),
        json!({"role": "user", "content": user_input}),
    ];

    // 持久化轨迹（user + 每轮 assistant + tool 消息）+ 跨轮累积的正文/思考/用量
    let mut persisted: Vec<Message> = vec![history::user_entry(user_input)];
    let mut accumulated_message = String::new();
    let mut accumulated_thought = String::new();
    let mut accumulated_usage = TokenUsage::default();

    let last_result = loop {
        // 取消检查：新一轮发起前已停止，则直接收尾（不再发请求）
        if cancel.load(Ordering::Relaxed) {
            break ChatResult {
                message: accumulated_message.clone(),
                thought: accumulated_thought.clone(),
                tool_calls: Vec::new(),
                usage: accumulated_usage.clone(),
                model: model_name.clone(),
            };
        }
        // 轮次边界：每轮大模型调用开始前发标记，前端据此封口上一轮气泡、开新气泡
        on_event(EngineEvent::RoundStart);
        let result = client
            .chat_stream(&messages, &tools_schema, cancel, |ev| match ev {
                StreamEvent::ThoughtDelta { delta } => on_event(EngineEvent::ThoughtDelta { delta }),
                StreamEvent::MessageDelta { delta } => on_event(EngineEvent::MessageDelta { delta }),
                StreamEvent::Done { .. } => {}
            })
            .await?;

        accumulated_message.push_str(&result.message);
        accumulated_thought.push_str(&result.thought);
        accumulated_usage.input_tokens += result.usage.input_tokens;
        accumulated_usage.output_tokens += result.usage.output_tokens;
        accumulated_usage.total_tokens += result.usage.total_tokens;

        // 持久化本轮 assistant 消息（中间 tool_calls 轮 / 最终正文轮都存）
        persisted.push(history::assistant_entry(&result));

        // 取消检查：本轮流式被中断，不再执行工具、不进入下一轮
        if cancel.load(Ordering::Relaxed) {
            break ChatResult {
                message: accumulated_message.clone(),
                thought: accumulated_thought.clone(),
                tool_calls: Vec::new(),
                usage: accumulated_usage.clone(),
                model: model_name.clone(),
            };
        }

        if result.tool_calls.is_empty() {
            break ChatResult {
                message: accumulated_message,
                thought: accumulated_thought,
                tool_calls: Vec::new(),
                usage: accumulated_usage,
                model: model_name.clone(),
            };
        }

        // 追加 assistant 消息（tool_calls 转 OpenAI 格式，arguments 序列化为字符串）
        messages.push(assistant_tool_calls_msg(&result));

        // 逐个执行工具，回填 role=tool，推前端事件，记精简记录
        for tc in &result.tool_calls {
            on_event(EngineEvent::ToolCall {
                id: tc.id.clone(),
                tool_name: tc.name.clone(),
                arguments: tc.arguments.clone(),
            });
            let out = tools.invoke_tool(&tc.name, &tc.arguments).await;
            let success = !tool_failed(&out);
            let summary = summarize(&out);
            on_event(EngineEvent::ToolResult {
                tool_name: tc.name.clone(),
                success,
                result: summary.clone(),
            });
            // 持久化 tool 消息（content 存精简摘要，注意力隔离；回填 LLM 仍用完整 out）
            persisted.push(history::tool_entry(&tc.id, &tc.name, tc.arguments.clone(), success, &summary));
            messages.push(json!({
                "role": "tool",
                "tool_call_id": tc.id,
                "content": out,
            }));
        }
    };

    // 每轮收尾：持久化完整轨迹 + 自动压缩检查（决策 D3：init/save 留给调用方）
    session.update_current_session(channel, &persisted)?;
    session.auto_compress_check(channel, &persisted).await?;

    // 旁路统计：失败只记日志，绝不把 chat 主流程带崩（决策 D5）
    if let Err(e) = usage.record_usage(&last_result.model, &last_result.usage) {
        eprintln!("[Usage] 记录 token 用量失败: {e}");
    }

    on_event(EngineEvent::Done { result: last_result });
    Ok(())
}

/// 把 ChatResult 的 tool_calls 转成 OpenAI assistant 消息（arguments 必须是 JSON 字符串）
fn assistant_tool_calls_msg(result: &ChatResult) -> Value {
    let tool_calls: Vec<Value> = result
        .tool_calls
        .iter()
        .map(|tc| {
            json!({
                "id": tc.id,
                "type": "function",
                "function": {
                    "name": tc.name,
                    "arguments": serde_json::to_string(&tc.arguments).unwrap_or_else(|_| "{}".to_string()),
                }
            })
        })
        .collect();
    json!({
        "role": "assistant",
        "content": result.message,
        "tool_calls": tool_calls,
    })
}

/// 精简工具结果：取首行 + 截断（供持久化，避免全量结果塞满会话历史）
fn summarize(result: &str) -> String {
    const MAX_LEN: usize = 200;
    let first_line = result.lines().next().unwrap_or("").trim();
    let s = if first_line.is_empty() {
        result.trim()
    } else {
        first_line
    };
    if s.chars().count() > MAX_LEN {
        let truncated: String = s.chars().take(MAX_LEN).collect();
        format!("{truncated}…")
    } else {
        s.to_string()
    }
}

/// 工具结果失败判定：fs_tools 的错误消息均以固定前缀开头（成功结果无固定前缀）。
/// 与 tool 模块的失败消息前缀保持同步，新增工具失败消息时需在此补充。
pub(crate) fn tool_failed(result: &str) -> bool {
    const FAIL_PREFIXES: &[&str] = &[
        "参数错误",
        "读取文件失败",
        "写入文件失败",
        "删除文件失败",
        "获取目录内容失败",
        "执行命令失败",
        "执行命令超时",
        "记忆检索失败",
        "记忆检索未配置",
        "未找到工具",
    ];
    FAIL_PREFIXES.iter().any(|p| result.starts_with(p))
}
