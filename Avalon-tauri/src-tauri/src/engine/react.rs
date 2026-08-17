// ReAct 双层循环编排
//
// 对话层（流式 chat_stream）→ 动作层（非流式 action）双层循环：
// 对话层产出 ChatResult 驱动「继续对话 / 进动作层」，动作层产出 ActionResult
// 驱动「调用工具 / 子分析 / 结束」。全部中间态经 on_event 发射，每轮收尾做会话持久化。
// 依赖 trait（ToolRegistry / SessionStore），llm 通过 LlmState 动态构建 client（配置热更新）。

#![allow(dead_code)]

use anyhow::Result;
use serde_json::Value;

use crate::config::ConfigStore;
use crate::llm::{ActionResult, ActionStep, LlmClient, LlmState, NextAction, StreamEvent};
use crate::prompt::{build_action_prompt, PromptAssembler};
use crate::session::{ActionRecord, ActionType, ChatMessage, SessionStore};
use crate::tool::ToolRegistry;

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
    on_event: &mut F,
) -> Result<()>
where
    F: FnMut(EngineEvent) + Send,
{
    let mut chat_history: Vec<ChatMessage> = vec![history::user_entry(user_input)];
    let tool_list = tools.get_tool_list();

    // loop 的 break 值携带最终对话结果（Stop 分支是唯一退出路径）
    let last_result = loop {
        // 每轮重读会话上下文（含最新压缩块）+ 动态构建 client（配置热更新）
        let session_context = session.get_context_for_prompt(channel)?;
        let system_prompt = prompt.assemble_chat_prompt(&tool_list, &session_context)?;
        let client = llm.client(config.get().llm);
        let history_str = serde_json::to_string(&chat_history)?;

        let chat_result = client
            .chat_stream(&system_prompt, user_input, &history_str, |ev| match ev {
                StreamEvent::ThoughtDelta { delta } => on_event(EngineEvent::ThoughtDelta { delta }),
                StreamEvent::MessageDelta { delta } => on_event(EngineEvent::MessageDelta { delta }),
                StreamEvent::Done { .. } => {}
            })
            .await?;

        let next = chat_result.next;
        chat_history.push(history::assistant_entry(&chat_result));

        match next {
            NextAction::Stop => break chat_result,
            NextAction::Action => {
                let target = chat_result.action_target.clone().unwrap_or_default();
                on_event(EngineEvent::ActionStart { target: target.clone() });
                let records = run_action_loop(&target, &tool_list, tools, &client, on_event).await?;
                chat_history.push(history::execution_record(records));
            }
        }
    };

    // 每轮收尾：持久化 + 自动压缩检查（决策 D3：init/save 留给调用方）
    session.update_current_session(channel, &chat_history)?;
    session.auto_compress_check(channel, &chat_history).await?;

    on_event(EngineEvent::Done { result: last_result });
    Ok(())
}

/// 动作层内循环：直到 Finished（无步数上限，提示词软规范，决策 D2）
async fn run_action_loop<F>(
    target: &str,
    tool_list: &str,
    tools: &dyn ToolRegistry,
    client: &LlmClient,
    on_event: &mut F,
) -> Result<Vec<ActionRecord>>
where
    F: FnMut(EngineEvent) + Send,
{
    let mut records: Vec<ActionRecord> = Vec::new();

    loop {
        let history_str = serde_json::to_string(&records)?;
        let system_prompt = build_action_prompt(target, tool_list, &history_str);
        let result: ActionResult = client.action(&system_prompt).await?;

        let analysis = result.analysis.clone();
        on_event(EngineEvent::ActionStep {
            analysis: analysis.clone(),
            next: result.next,
        });

        match result.next {
            ActionStep::ToolCall => {
                let (tool_name, arguments) = match &result.tool_call {
                    Some(tc) => (tc.name.clone(), tc.arguments.clone()),
                    None => (String::new(), Value::Null),
                };
                on_event(EngineEvent::ActionToolCall {
                    tool_name: tool_name.clone(),
                    arguments: arguments.clone(),
                });
                let tool_result = tools.invoke_tool(&tool_name, &arguments).await;
                let success = !tool_failed(&tool_result);
                on_event(EngineEvent::ActionToolResult {
                    tool_name: tool_name.clone(),
                    success,
                    result: tool_result.clone(),
                });
                records.push(history::action_record(
                    &result,
                    ActionType::ToolCall,
                    Some(tool_result),
                ));
            }
            ActionStep::SubAnalysis => {
                let sub_analysis = result.sub_analysis.clone().unwrap_or_default();
                on_event(EngineEvent::ActionSubAnalysis {
                    analysis: analysis.clone(),
                    sub_analysis,
                });
                records.push(history::action_record(&result, ActionType::SubAnalysis, None));
            }
            ActionStep::Finished => {
                on_event(EngineEvent::ActionFinished {
                    analysis: analysis.clone(),
                    token_usage: result.usage.clone(),
                });
                records.push(history::action_record(&result, ActionType::Finished, None));
                break;
            }
        }
    }

    Ok(records)
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
        "未找到工具",
    ];
    FAIL_PREFIXES.iter().any(|p| result.starts_with(p))
}
