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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompressedChunk {
    /// 块编号（普通 "2" / 合并 "merged_1_5"）。
    /// 自定义反序列化兼容 Python 的 int（普通）/ str（合并）混用，统一存 String。
    #[serde(default, deserialize_with = "deserialize_chunk")]
    pub chunk: String,
    #[serde(default)]
    pub summary: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// 仅合并块有：被合并的旧块编号列表
    #[serde(default)]
    pub merged_from_chunks: Vec<String>,

    // ---- 结构化摘要（旧数据缺失时为空，渲染层回退 summary） ----
    /// 稳定事实
    #[serde(default)]
    pub facts: Vec<String>,
    /// 已做出的决定
    #[serde(default)]
    pub decisions: Vec<String>,
    /// 未完成 / 待办
    #[serde(default)]
    pub open_tasks: Vec<String>,
    /// 工具执行的关键结果
    #[serde(default)]
    pub tool_outcomes: Vec<String>,
    /// 来源消息 id（本期未引入 Message.id，保留空）
    #[serde(default)]
    pub source_message_ids: Vec<String>,
    /// 覆盖的时间范围，如 "2026-11-01 ~ 2026-11-03"
    #[serde(default)]
    pub time_range: Option<String>,
    /// 压缩输入的估算 token 数
    #[serde(default)]
    pub token_count: Option<usize>,
}

impl CompressedChunk {
    /// 向量索引文本：结构化字段拼成可检索正文
    pub fn index_text(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for list in [
            &self.summary,
            &self.facts,
            &self.decisions,
            &self.open_tasks,
            &self.tool_outcomes,
        ] {
            for item in list {
                let item = item.trim();
                if !item.is_empty() {
                    parts.push(item.to_string());
                }
            }
        }
        parts.join("\n")
    }

    /// 无任何正文内容
    pub fn is_empty(&self) -> bool {
        self.summary.is_empty()
            && self.facts.is_empty()
            && self.decisions.is_empty()
            && self.open_tasks.is_empty()
            && self.tool_outcomes.is_empty()
    }
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

/// 会话请求统计（provider 返回的真实用量，作为下一次请求的锚点）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    /// 最近一次真实请求的 input_tokens
    #[serde(default)]
    pub last_input_tokens: usize,
    /// 最近一次真实请求的 output_tokens
    #[serde(default)]
    pub last_output_tokens: usize,
    /// 最近一次请求使用的模型
    #[serde(default)]
    pub last_model: String,
    /// 最近一次请求时间（RFC 3339）
    #[serde(default)]
    pub last_request_at: String,
}

impl Default for SessionStats {
    fn default() -> Self {
        Self {
            last_input_tokens: 0,
            last_output_tokens: 0,
            last_model: String::new(),
            last_request_at: String::new(),
        }
    }
}

/// 工具结果 artifact 引用：会话里只存预览，全文留在 artifacts/{session_id}/{id}.txt
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRef {
    /// 逻辑 id，写入预览末尾的 `[完整结果: artifact:{id}]`
    pub id: String,
    /// 全文的字符数
    pub chars: usize,
    /// 全文的字节数
    pub bytes: usize,
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
    /// 最近一次请求的统计与估算校准
    #[serde(default)]
    pub stats: SessionStats,
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
            stats: SessionStats::default(),
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
            stats: SessionStats::default(),
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
        /// 渠道来源元信息：飞书消息带 message_id / chat_id / chat_type / sender，
        /// 桌面端为 None 不落盘。投影为模型消息时由 context 层渲染成来源前缀。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        meta: Option<serde_json::Value>,
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
    /// 工具执行结果（对齐 OpenAI tool 消息；大结果 content 存预览，全文见 artifact）
    Tool {
        time: String,
        tool_call_id: String,
        name: String,
        /// 本次调用的参数（JSON 对象，自包含；旧数据缺失时反序列化为 Null）
        #[serde(default)]
        arguments: serde_json::Value,
        success: bool,
        content: String,
        /// 大体积结果落盘后的引用；短结果或旧数据为 None
        #[serde(default, skip_serializing_if = "Option::is_none")]
        artifact: Option<ArtifactRef>,
    },
}

/// 当前会话上下文用量（get_context_usage 命令返回）
#[derive(Debug, Clone, Serialize)]
pub struct ContextUsage {
    /// 下一次请求的输入锚点（最近真实 input + output）；旧会话回退到历史最大值
    pub used_tokens: usize,
    /// 当前预算触发线（input_budget × compress_trigger_ratio，受旧绝对阈值约束）
    pub threshold: usize,
}
