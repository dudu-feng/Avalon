// 飞书长连接的私有帧协议（pbbp2）
//
// 整个协议只有 Header / Frame 两个 message —— 定义抄自飞书官方 protobuf
// （对照 larksuite/oapi-sdk-python 的 lark_oapi/ws/pb/pbbp2.proto，
// 以及 lark-websocket-protobuf crate 的 prost 生成结果），字段编号与
// 可选性必须严格一致，否则服务端会解不出包。
//
// 注意是 proto2 语法：前四个字段是 required，其余 optional。

use prost::Message as _;

/// 帧类型，对应 Frame.method
pub const FRAME_CONTROL: i32 = 0;
pub const FRAME_DATA: i32 = 1;

/// header key 常量
pub const HEADER_TYPE: &str = "type";
pub const HEADER_MESSAGE_ID: &str = "message_id";
pub const HEADER_SUM: &str = "sum";
pub const HEADER_SEQ: &str = "seq";
pub const HEADER_TRACE_ID: &str = "trace_id";
pub const HEADER_BIZ_RT: &str = "biz_rt";

/// 握手失败时错误码所在的 HTTP 响应头
pub const HEADER_HANDSHAKE_STATUS: &str = "handshake-status";
pub const HEADER_HANDSHAKE_MSG: &str = "handshake-msg";
pub const HEADER_HANDSHAKE_AUTH_ERRCODE: &str = "handshake-autherrcode";

/// 消息类型，对应 DATA 帧 headers 里的 "type"
pub const MSG_EVENT: &str = "event";
pub const MSG_CARD: &str = "card";
pub const MSG_PING: &str = "ping";
pub const MSG_PONG: &str = "pong";

/// 端点协商返回码
pub const CODE_OK: i32 = 0;
pub const CODE_SYSTEM_BUSY: i32 = 1;
pub const CODE_FORBIDDEN: i32 = 403;
pub const CODE_AUTH_FAILED: i32 = 514;
pub const CODE_INTERNAL_ERROR: i32 = 1_000_040_343;
/// 超出连接数上限（单应用 50 条）——重试没有意义，必须停下来告诉用户
pub const CODE_EXCEED_CONN_LIMIT: i32 = 1_000_040_350;

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Header {
    #[prost(string, required, tag = "1")]
    pub key: String,
    #[prost(string, required, tag = "2")]
    pub value: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Frame {
    #[prost(uint64, required, tag = "1")]
    pub seq_id: u64,
    #[prost(uint64, required, tag = "2")]
    pub log_id: u64,
    #[prost(int32, required, tag = "3")]
    pub service: i32,
    #[prost(int32, required, tag = "4")]
    pub method: i32,
    #[prost(message, repeated, tag = "5")]
    pub headers: Vec<Header>,
    /// 类 HTTP 的 content-encoding，如 gzip / none
    #[prost(string, optional, tag = "6")]
    pub payload_encoding: Option<String>,
    /// 类 HTTP 的 content-type
    #[prost(string, optional, tag = "7")]
    pub payload_type: Option<String>,
    /// 类 HTTP 的 body
    #[prost(bytes = "vec", optional, tag = "8")]
    pub payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    pub log_id_new: Option<String>,
}

impl Frame {
    /// 按 key 取 header 值
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.key == key)
            .map(|h| h.value.as_str())
    }

    /// 取 header 并解析为整数，缺失或格式错误都返回默认值。
    /// sum/seq 这类字段缺失时按「非分片」处理最安全。
    pub fn header_int(&self, key: &str, default: i64) -> i64 {
        self.header(key)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(default)
    }

    /// 覆盖（或追加）一个 header
    pub fn set_header(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        match self.headers.iter_mut().find(|h| h.key == key) {
            Some(h) => h.value = value,
            None => self.headers.push(Header {
                key: key.to_string(),
                value,
            }),
        }
    }

    /// 序列化为待发送字节
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode_to_vec()
    }

    /// 构造心跳帧：CONTROL 类型 + type=ping
    pub fn ping(service: i32) -> Self {
        let mut frame = Self {
            seq_id: 0,
            log_id: 0,
            service,
            method: FRAME_CONTROL,
            headers: Vec::new(),
            payload_encoding: None,
            payload_type: None,
            payload: None,
            log_id_new: None,
        };
        frame.set_header(HEADER_TYPE, MSG_PING);
        frame
    }
}

/// 解析收到的二进制帧
pub fn decode_frame(bytes: &[u8]) -> Result<Frame, prost::DecodeError> {
    Frame::decode(bytes)
}
