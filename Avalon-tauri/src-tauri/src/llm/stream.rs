// 标记分隔流式状态机
//
// 把 chat 层的流式输出按三段标记切分：
//   <|thought|> ... </|thought|>   → 思考（流式推 ThoughtDelta）
//   <|message|> ... </|message|>   → 正文（流式推 MessageDelta）
//   <|control|> {...} </|control|> → 控制 JSON（累积后解析，驱动循环）
//
// 逐字流式的关键：每次只把「确定不是结束标记前缀」的内容发出，
// 保留尾部作标记前瞻，避免把半个标记当正文推给用户。

use serde::Deserialize;

use super::types::{ChatResult, NextAction, StreamEvent, TokenUsage};

const THOUGHT_END: &str = "</|thought|>";
const MESSAGE_END: &str = "</|message|>";
const CONTROL_END: &str = "</|control|>";
const MESSAGE_START: &str = "<|message|>";
const CONTROL_START: &str = "<|control|>";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Thought,
    Message,
    Control,
    Done,
}

pub struct StreamParser {
    phase: Phase,
    buffer: String,
    thought: String,
    message: String,
    control: String,
}

impl StreamParser {
    pub fn new() -> Self {
        Self {
            phase: Phase::Thought,
            buffer: String::new(),
            thought: String::new(),
            message: String::new(),
            control: String::new(),
        }
    }

    /// 推入一个增量，边解析边通过 emit 回调发出流式事件。
    pub fn push(&mut self, chunk: &str, emit: &mut dyn FnMut(StreamEvent)) {
        self.buffer.push_str(chunk);

        loop {
            let end_marker = match self.phase {
                Phase::Thought => THOUGHT_END,
                Phase::Message => MESSAGE_END,
                Phase::Control => CONTROL_END,
                Phase::Done => return,
            };

            match self.buffer.find(end_marker) {
                Some(idx) => {
                    // 找到完整结束标记：发出标记前内容，切换阶段
                    let content = self.buffer[..idx].to_string();
                    self.buffer.drain(..idx + end_marker.len());
                    self.emit_content(&content, emit);
                    self.advance();
                }
                None => {
                    // 未找到完整标记：发出安全前缀（保留尾部作前瞻）
                    let safe = self.buffer.len().saturating_sub(end_marker.len() - 1);
                    if safe > 0 {
                        let content = self.buffer[..safe].to_string();
                        self.buffer.drain(..safe);
                        self.emit_content(&content, emit);
                    }
                    return;
                }
            }
        }
    }

    /// 流结束后产出最终结果（flush 剩余缓冲 + 解析控制块 + 兜底）。
    pub fn finish(mut self) -> ChatResult {
        // flush 剩余缓冲（前瞻保留的尾部）
        if !self.buffer.is_empty() {
            let content = std::mem::take(&mut self.buffer);
            match self.phase {
                Phase::Thought => self.thought.push_str(&content),
                Phase::Message => self.message.push_str(&content),
                Phase::Control => self.control.push_str(&content),
                Phase::Done => {}
            }
        }

        // 兜底：模型未输出任何标记（整段当正文）
        if self.control.trim().is_empty() && self.message.is_empty() && !self.thought.is_empty() {
            self.message = std::mem::take(&mut self.thought);
        }

        let (next, action_target) = parse_control(&self.control);
        ChatResult {
            thought: self.thought,
            message: self.message,
            next,
            action_target,
            usage: TokenUsage::default(),
        }
    }

    fn emit_content(&mut self, content: &str, emit: &mut dyn FnMut(StreamEvent)) {
        if content.is_empty() {
            return;
        }
        match self.phase {
            Phase::Thought => {
                self.thought.push_str(content);
                emit(StreamEvent::ThoughtDelta {
                    delta: content.to_string(),
                });
            }
            Phase::Message => {
                self.message.push_str(content);
                emit(StreamEvent::MessageDelta {
                    delta: content.to_string(),
                });
            }
            Phase::Control => {
                self.control.push_str(content);
            }
            Phase::Done => {}
        }
    }

    fn advance(&mut self) {
        match self.phase {
            Phase::Thought => {
                self.phase = Phase::Message;
                self.skip_start(MESSAGE_START);
            }
            Phase::Message => {
                self.phase = Phase::Control;
                self.skip_start(CONTROL_START);
            }
            Phase::Control => {
                self.phase = Phase::Done;
            }
            Phase::Done => {}
        }
    }

    /// 跳过下一段的开始标记（连同前导空白）。模型未输出标记时跳过为空。
    fn skip_start(&mut self, start: &str) {
        if let Some(idx) = self.buffer.find(start) {
            let end = idx + start.len();
            self.buffer.drain(..end);
        }
    }
}

/// 解析控制块 JSON：`{"next":"action","action_target":"..."}`
fn parse_control(control: &str) -> (NextAction, Option<String>) {
    #[derive(Deserialize)]
    struct ControlBlock {
        next: Option<String>,
        #[serde(default)]
        action_target: Option<String>,
    }

    match serde_json::from_str::<ControlBlock>(control.trim()) {
        Ok(c) => {
            let next = match c.next.as_deref() {
                Some("action") => NextAction::Action,
                _ => NextAction::Stop,
            };
            (next, c.action_target)
        }
        Err(_) => (NextAction::Stop, None),
    }
}
