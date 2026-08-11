# 飞书渠道 — 会话记忆格式设计

## 架构定位

```
current/terminal.json    ─┐
current/feishu.json      ─┼── 独立当前会话，渠道隔离
current/web.json         ─┘

history/
  avalon_2026-06-06-23_03_31/   ─┐
  avalon_2026-08-11-15_30_00/   ─┼── 统一历史目录，共享记忆
  ...                           ─┘
```

- **当前会话**：每个渠道独立文件 `current/{channel}.json`，互不干扰
- **历史归档**：统一存入 `history/{session_id}/`，支持跨渠道记忆检索
- **核心引擎**：`agent/` 层不感知渠道差异，只读写标准消息条目

---

## 一、会话文件顶层结构

```json
{
  "id": "feishu_2026-08-11-15_30_00",
  "status": "active",
  "compress_round": 2,
  "compressed": [
    {
      "chunk": 1,
      "summary": ["用户在终端上编写了一个批量文件重命名脚本"],
      "keywords": ["Python", "文件重命名", "脚本"]
    }
  ],
  "super_compressed": {},
  "session": [
    {
      "role": "user",
      "time": "2026-08-11-15:30:00",
      "content": "上次那个重命名脚本，能加个按日期排序的功能吗？",
      "meta": { ... }
    },
    {
      "role": "assistant",
      "time": "2026-08-11-15:30:08",
      "content": "当然可以，在排序逻辑那里加个按创建日期排序的选项...",
      "thought": "用户通过飞书追问之前的脚本功能",
      "token_usage": { ... },
      "meta": { ... }
    }
  ]
}
```

与现有格式对比：**在原有 `{role, time, content}` 结构上，仅新增一个 `meta` 字段**。`compress_round`、`compressed`、`super_compressed` 完全沿用现有机制。

---

## 二、消息条目格式

### 2.1 通用结构

所有消息条目在现有基础上增加 `meta` 字段：

```
{
  "role": "user" | "assistant" | "system",
  "time": "yyyy-MM-dd-HH:mm:ss",
  "content": "...",            // LLM 阅读的主体文本（已有）
  "meta": { ... }              // 渠道结构化元数据（新增）
}
```

- `role: "user"` — 来自飞书的入站消息 / 卡片交互
- `role: "assistant"` — Avalon 的回复 / 执行记录
- `role: "system"` — 飞书系统事件（bot 入群、退群等）

### 2.2 `content` 字段 —— 对 LLM 自解释

`content` 是 LLM 唯一需要阅读的文本字段。飞书 Adapter 负责将消息的渠道上下文**编码进 `content`**，让 LLM 无需解析 `meta` 即可理解对话背景。

**拼接规则：**

#### 单聊 (p2p)

```
原始: content_text = "帮我查天气"
  ↓
content: "帮我查天气"
```

单聊不额外添加前缀——用户就是对话的另一方。

#### 群聊，有人 @了机器人

```
原始: sender_name="张三", content_text="帮我查一下今天的天气", mentioned_bot=true
  ↓
content: "张三: 帮我查一下今天的天气"
```

`发送者名称: ` 前缀标识谁在说话。群聊中只有 @了机器人的消息才会进入处理管线（由 `PolicyConfig.require_mention` 控制）。

#### 群聊，有回复上下文

```
原始: sender_name="张三", content_text="那排期呢？",
      reply_to={sender_name:"李四", snippet:"建议方案A，预计两周开发"}
  ↓
content: "张三: 那排期呢？
> 回复 李四: 建议方案A，预计两周开发"
```

用 `>` 引用块表示被回复的消息内容。

#### 多模态消息（图片/文件）

```
原始: content_text="帮我看看这个报错",
      resources=[{type:"image", file_name:"error_screenshot.png", size_bytes:245760}]
  ↓
content: "张三: 帮我看看这个报错
[图片: error_screenshot.png, 245KB]"
```

当前 LLM 尚不支持视觉理解，`[图片: ...]` 作为占位提示。未来接入多模态模型后，可配合 `meta.resources[].file_key` 下载原图传入 vision API。

#### 卡片交互

```
原始: CardActionEvent(action={tag:"button", value:'{"act":"approve"}', option:"同意"})
  ↓
content: "张三: [点击了卡片按钮: 同意]"
```

#### 系统事件

```
原始: BotAddedEvent(chat_name="Avalon测试群", operator_name="管理员")
  ↓
content: "[系统] 被 管理员 邀请加入群聊 Avalon测试群"
```

### 2.3 `meta` 字段 —— 对程序结构化

`meta` 承载渠道专有的结构化元数据，供 Adapter 层在回复/编辑/撤回等操作中使用。核心引擎不解析 `meta`。

#### 用户消息 meta

```json
{
  "message_id": "om_abc123def456",
  "chat_id": "oc_26b66a5eb603162b849f91bcd8815b20",
  "chat_type": "group",
  "sender": {
    "open_id": "ou_7dede290d6a27698b969a7fd70ca53da",
    "display_name": "张三"
  },
  "mentions": [
    {
      "key": "ou_bot_xxx",
      "name": "Avalon",
      "mentioned_bot": true
    }
  ],
  "reply_to": {
    "message_id": "om_prev456",
    "sender_name": "李四",
    "snippet": "建议方案A，预计两周开发"
  },
  "content_type": "text",
  "resources": []
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|:--:|------|
| `message_id` | string | ✅ | 飞书消息唯一 ID，用于去重 key |
| `chat_id` | string | ✅ | 会话 ID，回复消息的定位目标 |
| `chat_type` | enum | ✅ | `p2p` / `group` / `topic` |
| `sender.open_id` | string | ✅ | 发送者 open_id |
| `sender.display_name` | string | - | 发送者展示名（懒加载后填充） |
| `mentions` | array | - | 结构化 @ 列表 |
| `mentions[].key` | string | - | 被 @ 者的 open_id 或 user_id |
| `mentions[].name` | string | - | 被 @ 者的展示名 |
| `mentions[].mentioned_bot` | bool | - | 是否 @了当前机器人 |
| `reply_to` | object\|null | - | 被回复消息的摘要，null 表示非回复 |
| `reply_to.message_id` | string | - | 被回复消息 ID |
| `reply_to.sender_name` | string | - | 被回复消息的发送者 |
| `reply_to.snippet` | string | - | 被回复消息的内容摘要（截断至 100 字） |
| `content_type` | enum | ✅ | `text` / `post` / `image` / `file` / `audio` / `video` / `mixed` / `card_action` |
| `resources` | array | - | 非文本资源描述列表 |

**resources 子结构：**

```json
{
  "type": "image",
  "file_key": "img_v3_xxx",
  "file_name": "screenshot.png",
  "size_bytes": 245760,
  "image_info": { "width": 1920, "height": 1080 }
}
```

| 字段 | 说明 |
|------|------|
| `type` | `image` / `file` / `audio` / `video` / `sticker` |
| `file_key` | 飞书文件 key，用于 `download_resource()` 下载 |
| `file_name` | 原始文件名 |
| `size_bytes` | 文件大小 |
| `image_info` | 仅 image 类型，宽高信息 |

#### 助手回复 meta

```json
{
  "channel": "feishu",
  "chat_id": "oc_xxx",
  "reply_to": "om_abc123def456",
  "message_type": "markdown",
  "sent_message_id": "om_sent_xxx"
}
```

| 字段 | 说明 |
|------|------|
| `channel` | 固定 `"feishu"`，标记回复通过哪个渠道发出 |
| `chat_id` | 目标会话 |
| `reply_to` | 被回复消息的 `message_id`（引用回复） |
| `message_type` | 发送类型：`text` / `markdown` / `card` |
| `sent_message_id` | 发送成功后飞书返回的消息 ID，供后续编辑/撤回/更新卡片 |

#### 卡片交互 meta

```json
{
  "chat_id": "oc_xxx",
  "chat_type": "p2p",
  "sender": { "open_id": "ou_xxx", "display_name": "张三" },
  "event_type": "card_action",
  "action": {
    "tag": "button",
    "value": "{\"action\": \"confirm_execute\"}",
    "option": "确认执行"
  },
  "source_message_id": "om_card_xxx"
}
```

#### 系统事件 meta

```json
{
  "chat_id": "oc_xxx",
  "event_type": "bot_added",
  "operator": { "open_id": "ou_admin", "display_name": "管理员" }
}
```

`event_type` 枚举：`bot_added` / `bot_leave` / `message_read` / `reaction`

---

## 三、与现有系统的兼容

### 3.1 会话管理（session_manage.py）

```
init_session(channel)          →  零改动 ✅  (仍按 channel 读写 current 文件)
update_current_session(history, channel)  →  零改动 ✅  (history 条目结构不变)
session_compress(channel)      →  零改动 ✅  (读取 session[] 数组，压缩 content)
get_session_context_for_prompt(channel) →  零改动 ✅  (裁剪逻辑不变)
```

### 3.2 LLM 调用（llm.py）

```
llm_chat(user_input, chat_history, channel)  →  零改动 ✅
  - system_prompt 中追加的 current_session 会自然包含 meta 字段
  - LLM 能看到 meta 的 JSON，但主要依据 content 理解对话
  - content 已自包含发送者/群聊上下文
```

### 3.3 ReAct 循环

```
streaming_react_loop(user_input, on_event, channel)  →  零改动 ✅
  - chat_history 条目格式不变
  - 新增的 meta 字段仅在持久化时带入，不影响循环逻辑
```

### 3.4 压缩管道

```
llm_compress(session_data)  →  零改动 ✅
  - 压缩模型的输入是 content 的集合
  - 飞书消息的 content 自带 [发送者] / 回复上下文 / 媒体标记
  - 压缩结果 (summary + keywords) 自然包含跨渠道上下文
```

### 3.5 ZVec 向量检索

```
zvec_store.insert_session_memory(doc_id, summary, keywords, timestamp)  →  零改动 ✅
  - 压缩摘要的向量化与检索逻辑完全不变
  - 飞书消息压缩后的摘要和其他渠道一样进入统一向量库
```

---

## 四、Adapter 层职责

飞书 Adapter（新增文件 `server/services/feishu_adapter.py`）承担以下转换职责：

```
飞书事件                    会话条目                    飞书回复
────────                   ────────                   ────────
InboundMessage  ──转换──>  {role, time,               SendResult
CardActionEvent            content, meta}              stream()
BotAddedEvent                                          update_card()
ReactionEvent              session_manage              edit_message()
...                        .update_current_session()   ...
```

**Adapter 的核心方法：**

| 方法 | 输入 | 输出 | 职责 |
|------|------|------|------|
| `to_session_entry(msg: InboundMessage)` | 飞书入站消息 | `{role, time, content, meta}` | 拼接 content、构造 meta |
| `to_reply_target(meta: dict)` | 消息 meta | `(chat_id, reply_to_msg_id)` | 提取回复定位信息 |
| `handle_message(msg)` | 飞书消息 | — | 转换 → 送入 ReAct 循环 → 回复 |
| `handle_card_action(event)` | 卡片事件 | — | 转换 → 送入 ReAct 循环 → 更新卡片 |
| `handle_system_event(event)` | 系统事件 | — | 记录 system 条目 |

---

## 五、`content` 拼接参考实现

```python
def build_content(msg: InboundMessage) -> str:
    """将飞书 InboundMessage 转换为 LLM 可读的 content 字符串。"""
    parts = []

    # 1. 发送者前缀（仅群聊需要）
    if msg.chat_type == "group" and msg.sender_name:
        parts.append(f"{msg.sender_name}: ")

    # 2. 消息正文
    if msg.content_text:
        parts.append(msg.content_text)
    else:
        # 纯媒体消息，用资源描述代替
        parts.append("[消息]")

    # 3. 回复上下文（被回复消息的引用）
    if has_reply_context(msg):
        r = msg.reply_context
        parts.append(f"\n> 回复 {r.sender_name}: {r.snippet}")

    # 4. 非文本资源标记
    for res in (msg.resources or []):
        label = _resource_label(res)
        parts.append(f"\n[{label}]")

    return "".join(parts)


def _resource_label(res) -> str:
    """生成资源的人类可读标签。"""
    type_labels = {
        "image": "图片", "file": "文件",
        "audio": "语音", "video": "视频",
        "sticker": "表情包",
    }
    label = type_labels.get(res.type, "附件")
    if res.file_name:
        label += f": {res.file_name}"
    if res.size_bytes:
        label += f", {_format_size(res.size_bytes)}"
    return label


def _format_size(size: int) -> str:
    if size < 1024:
        return f"{size}B"
    elif size < 1024 * 1024:
        return f"{size // 1024}KB"
    else:
        return f"{size // (1024*1024):.1f}MB"
```

---

## 六、完整示例：一条群聊消息的转换

```
═══════════════════════════════════════════════════════════════
飞书推送的原始事件 (InboundMessage)
═══════════════════════════════════════════════════════════════
message_id:     "om_2c6e5a8b9d01f3a7"
chat_id:        "oc_26b66a5eb603162b849f91bcd8815b20"
chat_type:      "group"
sender.open_id: "ou_7dede290d6a27698b969a7fd70ca53da"
sender_name:    "张三"
content_text:   "@Avalon 上次说的那个重命名脚本，能加个按文件日期排序的功能吗"
mentions:       [{key: "ou_bot_xxx", name: "Avalon", mentioned_bot: true}]
reply_to:       {
  message_id:   "om_1a2b3c4d",
  sender_name:  "李四",
  snippet:      "我觉得这个重命名工具挺实用的，可以考虑再加个按日期分组的功能"
}
content_type:   "text"
resources:      []

═══════════════════════════════════════════════════════════════
转换后的会话条目
═══════════════════════════════════════════════════════════════
{
  "role": "user",
  "time": "2026-08-11-15:30:15",
  "content": "张三: @Avalon 上次说的那个重命名脚本，能加个按文件日期排序的功能吗
> 回复 李四: 我觉得这个重命名工具挺实用的，可以考虑再加个按日期分组的功能",

  "meta": {
    "message_id": "om_2c6e5a8b9d01f3a7",
    "chat_id": "oc_26b66a5eb603162b849f91bcd8815b20",
    "chat_type": "group",
    "sender": {
      "open_id": "ou_7dede290d6a27698b969a7fd70ca53da",
      "display_name": "张三"
    },
    "mentions": [
      {"key": "ou_bot_xxx", "name": "Avalon", "mentioned_bot": true}
    ],
    "reply_to": {
      "message_id": "om_1a2b3c4d",
      "sender_name": "李四",
      "snippet": "我觉得这个重命名工具挺实用的，可以考虑再加个按日期分组的功能"
    },
    "content_type": "text",
    "resources": []
  }
}

═══════════════════════════════════════════════════════════════
LLM 实际看到的内容 (系统提示 + 会话上下文 + 本条消息)
═══════════════════════════════════════════════════════════════
...（系统提示）...
=====历史会话记录(feishu.json)=====
...（压缩摘要）...

当前对话:
用户: 张三: @Avalon 上次说的那个重命名脚本，能加个按文件日期排序的功能吗
> 回复 李四: 我觉得这个重命名工具挺实用的，可以考虑再加个按日期分组的功能

(LLM 能理解: 张三在群里 @了 Avalon，他在回复李四关于重命名工具的讨论，
 这是一个跨渠道的追问——"上次说的"指向 terminal 渠道的历史会话)
```

---

## 七、设计约束与注意事项

| 约束 | 措施 |
|------|------|
| `content` 不应包含敏感信息 | `meta.sender.open_id` 等敏感标识只放在 meta 中，不写入 content |
| `meta` 可能膨胀 session 文件大小 | `meta` 仅用户消息完整保留；助手消息只保留 `chat_id` + `reply_to` + `sent_message_id` |
| 压缩模型应忽略 `meta` | 压缩时只需取 `session[].content`，meta 跳过 |
| 飞书 3 秒超时 | `_handle_message` 应立即返回 200，LLM 处理异步进行 |
| 消息去重 | `meta.message_id` 作为业务层去重 key，弥补 SDK 内置去重在跨重启场景的不足 |
| open_id → 展示名映射 | 首次收到某用户消息时通过 API 获取展示名，缓存到内存 dict；每次更新 content 前从缓存查询 |
