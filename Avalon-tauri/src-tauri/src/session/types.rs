// 会话模块数据结构定义
//
// SessionData 是会话文件的 JSON 结构（current/{channel}.json 与 history/{id}/index.json 的内容）；
// ChatMessage / ActionRecord 是消息与动作记录（记忆落库的载体，engine 产生、session 持久化）。

#![allow(dead_code)] // session 模块供未来 engine/tool 引用，当前无调用方，接入后移除

use serde::{Deserialize, Deserializer, Serialize};

use crate::llm::{TokenUsage, ToolCall};

/// 会话状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    Inactive,
    Archived,
}

/// 压缩块：普通块 chunk="2"，合并块 chunk="merged_1_5"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedChunk {
    /// 块编号（普通 "2" / 合并 "merged_1_5"）。
    /// 自定义反序列化兼容 Python 的 int（普通）/ str（合并）混用，统一存 String。
    #[serde(deserialize_with = "deserialize_chunk")]
    pub chunk: String,
    pub summary: Vec<String>,
    pub keywords: Vec<String>,
    /// 仅合并块有：被合并的旧块编号列表
    #[serde(default)]
    pub merged_from_chunks: Vec<String>,
}

/// chunk 字段反序列化：接受 int 或 str，统一转 String（读旧 Python 数据兼容）
fn deserialize_chunk<'de, D>(d: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Int(i64),
        Str(String),
    }
    match Raw::deserialize(d)? {
        Raw::Int(n) => Ok(n.to_string()),
        Raw::Str(s) => Ok(s),
    }
}

/// super_compressed 反序列化：null / 空数组 []（Python 旧数据）/ 对象 → Option<CompressedChunk>
fn deserialize_super_compressed<'de, D>(d: D) -> Result<Option<CompressedChunk>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(d)?;
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Array(arr) if arr.is_empty() => Ok(None),
        other => match serde_json::from_value::<CompressedChunk>(other) {
            Ok(chunk) => Ok(Some(chunk)),
            Err(e) => Err(serde::de::Error::custom(e)),
        },
    }
}

/// 会话文件结构（current/{channel}.json 与 history/{id}/index.json 的内容）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub id: String,
    pub status: SessionStatus,
    /// 已压缩轮数（下一普通块的 chunk 编号）
    #[serde(default)]
    pub compress_round: usize,
    #[serde(default)]
    pub compressed: Vec<CompressedChunk>,
    /// 渐进式总结产出的超级摘要（0 或 1 个）
    /// 反序列化兼容 Python 旧数据的空列表 `[]`（等价 None）
    #[serde(default, deserialize_with = "deserialize_super_compressed")]
    pub super_compressed: Option<CompressedChunk>,
    /// 当前未压缩的消息
    #[serde(default)]
    pub session: Vec<ChatMessage>,
}

impl SessionData {
    /// 空会话（inactive，未初始化 / 归档重置）
    pub fn empty() -> Self {
        Self {
            id: String::new(),
            status: SessionStatus::Inactive,
            compress_round: 0,
            compressed: Vec::new(),
            super_compressed: None,
            session: Vec::new(),
        }
    }

    /// 新建活跃会话（id = {channel}_{timestamp}）
    pub fn new_active(id: String) -> Self {
        Self {
            id,
            status: SessionStatus::Active,
            compress_round: 0,
            compressed: Vec::new(),
            super_compressed: None,
            session: Vec::new(),
        }
    }
}

/// 消息角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

/// 一条对话消息（持久化 + 前端展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub time: String,
    pub content: String,
    #[serde(default)]
    pub thought: Option<String>,
    #[serde(default)]
    pub token_usage: TokenUsage,
    /// 动作层执行记录（仅 assistant 且触发 action 时存在）
    #[serde(default)]
    pub action_history: Option<Vec<ActionRecord>>,
}

/// 动作步骤类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionType {
    ToolCall,
    SubAnalysis,
    Finished,
    Error,
}

/// 动作层执行记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecord {
    pub action_type: ActionType,
    pub time: String,
    pub analysis: String,
    #[serde(default)]
    pub tool_call: Option<ToolCall>,
    #[serde(default)]
    pub tool_result: Option<String>,
    #[serde(default)]
    pub sub_analysis: Option<String>,
    /// action 步骤的 token 用量（对齐 Python action_result 的 token_usage，供 auto_compress_check 遍历）
    #[serde(default)]
    pub token_usage: TokenUsage,
}

/// 限界会话上下文（get_context_for_prompt 的输出，序列化为 JSON 拼进 system_prompt）
#[derive(Debug, Clone, Serialize)]
pub struct SessionContext {
    pub id: String,
    pub status: SessionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub super_compressed: Option<CompressedChunk>,
    /// 最近 context_chunks 个普通压缩块
    pub compressed: Vec<CompressedChunk>,
    /// 当前未压缩消息
    pub session: Vec<ChatMessage>,
    /// 裁剪掉的旧块数（超限时存在）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub older_chunks_omitted: Option<usize>,
}
