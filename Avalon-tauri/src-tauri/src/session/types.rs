// 会话模块数据结构定义
//
// SessionData 是会话文件的 JSON 结构（current/{channel}.json 与 history/{id}/index.json 的内容）；
// Message 是判别联合消息（user/assistant/tool 三种形态，对齐 OpenAI 消息模型），是记忆落库的载体。

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
    /// 会话标题（归档时由首条 user 消息截断生成，空则前端回退时间戳）
    #[serde(default)]
    pub title: String,
    /// 已压缩轮数（下一普通块的 chunk 编号）
    #[serde(default)]
    pub compress_round: usize,
    #[serde(default)]
    pub compressed: Vec<CompressedChunk>,
    /// 渐进式总结产出的超级摘要（0 或 1 个）
    /// 反序列化兼容 Python 旧数据的空列表 `[]`（等价 None）
    #[serde(default, deserialize_with = "deserialize_super_compressed")]
    pub super_compressed: Option<CompressedChunk>,
    /// 当前未压缩的消息（user/assistant/tool 平铺）
    #[serde(default)]
    pub messages: Vec<Message>,
}

impl SessionData {
    /// 空会话（inactive，未初始化 / 归档重置）
    pub fn empty() -> Self {
        Self {
            id: String::new(),
            status: SessionStatus::Inactive,
            title: String::new(),
            compress_round: 0,
            compressed: Vec::new(),
            super_compressed: None,
            messages: Vec::new(),
        }
    }

    /// 新建活跃会话（id = {channel}_{timestamp}）
    pub fn new_active(id: String) -> Self {
        // 初始标题 = id 的时间戳部分（去掉 {channel}_ 前缀），归档时若未手动改名则被首条消息覆盖
        let title = id.splitn(2, '_').nth(1).unwrap_or(id.as_str()).to_string();
        Self {
            id,
            status: SessionStatus::Active,
            title,
            compress_round: 0,
            compressed: Vec::new(),
            super_compressed: None,
            messages: Vec::new(),
        }
    }
}

/// 会话列表元信息（list_sessions 返回）
#[derive(Debug, Clone, Serialize)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub status: SessionStatus,
    /// 创建时间（epoch 秒，由 id 时间戳解析，供前端时间分组）
    pub created_at: i64,
}

/// 渐进式加载历史块的结果（load_session_history 命令返回）
#[derive(Debug, Clone, Serialize)]
pub struct LoadHistoryResult {
    /// 本次返回的块号；None 表示无更早历史可加载
    pub chunk: Option<u64>,
    pub messages: Vec<Message>,
    /// 是否还有比本块更早的块（前端据此决定是否继续显示加载入口）
    pub has_earlier: bool,
}

/// 一条会话消息（判别联合：user/assistant/tool 结构各异，对齐 OpenAI 消息模型）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    /// 用户输入（只有正文，无思考/用量）
    User {
        time: String,
        content: String,
    },
    /// 智能体回复（对齐 OpenAI assistant 消息：正文 + 思考 + 工具调用 + 用量）
    Assistant {
        time: String,
        content: String,
        /// 思考过程（DeepSeek reasoning_content，非空才落盘）
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        /// 本轮发起的工具调用（无则省略；arguments 为 JSON 对象）
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ToolCall>>,
        #[serde(default)]
        token_usage: TokenUsage,
        /// 生成该回复的模型名（供各模型 token 报表归集）
        #[serde(default)]
        model: String,
    },
    /// 工具执行结果（对齐 OpenAI tool 消息，content 为精简摘要，arguments 自包含供前端独立渲染）
    Tool {
        time: String,
        tool_call_id: String,
        name: String,
        /// 本次调用的参数（JSON 对象，自包含；旧数据缺失时反序列化为 Null）
        #[serde(default)]
        arguments: serde_json::Value,
        success: bool,
        content: String,
    },
}

/// 当前会话上下文用量（get_context_usage 命令返回）
#[derive(Debug, Clone, Serialize)]
pub struct ContextUsage {
    /// 当前会话最大输入 token（遍历 assistant 消息的 token_usage.input_tokens 取最大）
    pub used_tokens: usize,
    /// 压缩阈值（config.session_memory.compress_threshold）
    pub threshold: usize,
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
    pub messages: Vec<Message>,
    /// 裁剪掉的旧块数（超限时存在）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub older_chunks_omitted: Option<usize>,
}
