# 飞书渠道 — 接入架构设计

## 一、架构总览

```
═══════════════════════════════════════════════════════════════════════════
                              飞书开放平台
═══════════════════════════════════════════════════════════════════════════
                                      │
                    WebSocket (长连接) / HTTP (Webhook)
                                      │
══════════════════════════════════════╪════════════════════════════════════
                         lark_service  │  (新增)
══════════════════════════════════════╪════════════════════════════════════
                                      │
  ┌───────────────────────┐  ┌────────┴────────┐  ┌──────────────────────┐
  │   channel_manager.py  │  │ event_handler.py │  │   reply_adapter.py   │
  │                       │  │                  │  │                      │
  │  · 初始化 Channel     │  │ · 注册事件回调   │  │ · chat_message →     │
  │  · 启动/停止生命周期  │  │ · 线程池派发     │  │   channel.send()     │
  │  · 重连与健康检查     │  │ · 超时保护       │  │ · chat_thought →     │
  │                       │  │ · 异常边界       │  │   流式卡片更新       │
  └───────────┬───────────┘  └────────┬────────┘  │ · action_* →         │
              │                       │           │   工具结果回复        │
              │                       │           └──────────┬───────────┘
              │                       │                      │
              │              ┌────────┴────────┐             │
              │              │message_converter│             │
              │              │                 │             │
              │              │ InboundMessage  │             │
              │              │      →          │             │
              │              │ session entry   │             │
              │              └────────┴────────┘             │
              │                       │                      │
══════════════╪═══════════════════════╪══════════════════════╪════════════
   server/   │                       │                      │
══════════════╪═══════════════════════╪══════════════════════╪════════════
              │                       ▼                      │
              │           ┌───────────────────┐              │
              │           │  chat_service.py  │              │
              │           │                   │◄─────────────┘
              │           │ generate_sse()    │  (SSE 路径复用)
              │           │ streaming_react_  │
              │           │ loop()            │
              │           └────────┬──────────┘
              │                    │
══════════════╪════════════════════╪══════════════════════════════════════
   agent/     │                    │
══════════════╪════════════════════╪══════════════════════════════════════
              │                    ▼
              │           ┌───────────────────┐
              └───────────┤  session_manage   │  (channel="feishu")
                          │  llm_chat         │
                          │  base_tool        │
                          │  zvec_store       │
                          └───────────────────┘
```

**设计原则：**

| # | 原则 | 说明 |
|---|------|------|
| 1 | **agent/ 零侵入** | 飞书渠道作为 server 层的 Adapter，不修改 agent 核心引擎 |
| 2 | **复用 ReAct 管线** | 直接使用 `streaming_react_loop()`，通过 `on_event` 回调驱动飞书回复 |
| 3 | **异步桥接** | 飞书 Channel 是 async 的，ReAct 是 sync 的，中间用 `asyncio.Queue` + 线程池桥接 |
| 4 | **快速响应** | 收到消息后立即返回 200（WS 模式内置），LLM 处理结果异步送达到飞书 |

---

## 二、模块划分

```
server/lark_service/              ← 飞书渠道 Adapter（新增目录）
├── __init__.py                   ← 导出公共接口
├── config.py                     ← 飞书配置项读取
├── channel_manager.py            ← FeishuChannel 生命周期
├── event_handler.py              ← 飞书事件 → ReAct 循环调度
├── message_converter.py          ← InboundMessage → session entry
└── reply_adapter.py              ← ReAct 事件 → channel.send/stream
```

### 2.1 模块职责

| 模块 | 输入 | 输出 | 职责 |
|------|------|------|------|
| `config.py` | `.env` | `FeishuConfig` dataclass | 读取飞书凭据和策略参数 |
| `channel_manager.py` | `FeishuConfig` | `FeishuChannel` 实例 | 创建、启动、停止 Channel；健康检查；重连 |
| `event_handler.py` | `InboundMessage` / `CardActionEvent` | — | 路由事件类型；调用 converter；启动线程池执行 ReAct；驱动 reply_adapter |
| `message_converter.py` | `InboundMessage` | `{role, time, content, meta}` | 将飞书消息转为标准 session entry |
| `reply_adapter.py` | ReAct 事件流 + `FeishuChannel` | — | 将 `chat_message`/`chat_thought`/`action_*` 转为飞书消息 |

### 2.2 模块间依赖

```
config.py          ← 无依赖 (只读 .env)

message_converter.py ← 无依赖 (纯函数转换)

reply_adapter.py   ← 依赖 FeishuChannel 实例 (注入)
                     依赖 message_converter (格式引用)

event_handler.py   ← 依赖 message_converter
                     依赖 reply_adapter
                     依赖 chat_service.streaming_react_loop

channel_manager.py ← 依赖 config, event_handler
                     创建 FeishuChannel 并装配

server/main.py     ← 依赖 channel_manager (lifespan 中启动/停止)
```

---

## 三、核心数据流

### 3.1 完整消息处理流程

```
时间线 ──────────────────────────────────────────────────────────────────>

[飞书平台]                    [lark_service]                   [agent/核心]
    │                              │                              │
    │  (WebSocket 推送消息事件)      │                              │
    │ ─────── InboundMessage ──────>│                              │
    │                              │                              │
    │                    event_handler.handle_message(msg)         │
    │                              │                              │
    │                    ① 转换消息                                │
    │                    entry = converter.to_session_entry(msg)    │
    │                              │                              │
    │                    ② 初始化 reply_adapter                    │
    │                    adapter.start(msg.chat_id)                 │
    │                              │                              │
    │                    ③ 启动异步消费者                           │
    │                    loop.create_task(consume(adapter))         │
    │                              │                              │
    │                    ④ 线程池执行 ReAct ───────────────────>   │
    │                              │      streaming_react_loop()   │
    │                              │        user_input=content     │
    │                              │        channel="feishu"       │
    │                              │        on_event=callback      │
    │                              │            │                  │
    │                              │      [对话层]                  │
    │                              │      llm_chat() →             │
    │                              │        chat_thought ──┐       │
    │                              │        chat_message ──┤       │
    │                              │            │           │       │
    │  ◄──── channel.send/stream ──│ ⑤ 异步消费者处理 ◄────┘       │
    │         (回复到飞书)          │   chat_thought → (忽略)       │
    │                              │   chat_message → stream/reply │
    │                              │   chat_stop → finish          │
    │                              │   action_* → 工具结果消息     │
    │                              │   error → 错误通知            │
    │                              │            │                  │
    │                              │      [持久化]                  │
    │                              │      session_manage.          │
    │                              │        update_current_session │
    │                              │        auto_compress_check    │
```

### 3.2 关键时序约束

```
0ms    收到飞书消息
       │  FeishuChannel 内部控制，SDK 立即 ACK
       │  (WebSocket 模式: 3秒内处理完成且不抛异常)
       │
~1ms   event_handler 收到 msg
       │  ① 转换消息 (~0.1ms)
       │  ② 发送占位回复 "思考中..." (~200ms)
       │  ③ 启动线程池执行 ReAct
       │
~200ms 飞书用户看到 "思考中..."
       │
~5-30s LLM 返回 chat_message
       │  ④ on_event 跨线程推入 asyncio.Queue
       │  ⑤ 消费者取出事件 → channel.stream() / channel.send()
       │
~30s   飞书用户看到完整回复
```

### 3.3 线程与异步桥接

```
┌─────────────────────────────────────────────────────────────────┐
│  AsyncIO Event Loop (FeishuChannel 的事件循环)                   │
│                                                                  │
│  asyncio task: handle_message(msg)                              │
│    │                                                             │
│    ├─ converter.to_session_entry(msg)        # 同步，极快        │
│    ├─ channel.send(placeholder)              # 异步              │
│    ├─ asyncio.create_task(consumer)          # 启动消费者        │
│    │                                                             │
│    └─ await loop.run_in_executor(             # 跨线程派发       │
│         None,                                                     │
│         _run_react_in_thread,                                     │
│         user_input, channel, queue     ← 传入 Queue 用于回调     │
│       )                                                          │
│                                                                  │
├ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┤
│  Thread Pool (ReAct 同步执行)                                     │
│                                                                  │
│  _run_react_in_thread(user_input, channel, queue):               │
│    def on_event(event_type, data):                               │
│        loop.call_soon_threadsafe(queue.put_nowait, (t, d))       │
│                                                                  │
│    streaming_react_loop(user_input, on_event, channel)           │
│      ├─ llm_chat()        ──→ chat_thought ──→ queue            │
│      │                       chat_message ──→ queue              │
│      ├─ llm_action()      ──→ action_step  ──→ queue            │
│      │                       tool_result  ──→ queue              │
│      └─ session_manage    ──→ done         ──→ queue            │
│                                                                  │
├ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┤
│  AsyncIO Event Loop (消费者)                                     │
│                                                                  │
│  async def _consume(queue, adapter):                             │
│      while True:                                                 │
│          event_type, data = await queue.get()                    │
│          if event_type == "done": break                          │
│          await adapter.handle(event_type, data)                  │
│                                                                  │
│          e.g. chat_message → await channel.stream(...)           │
│               action_finished → await channel.send(result)       │
│               error → await channel.send(error_msg)              │
└─────────────────────────────────────────────────────────────────┘
```

**为什么用 Queue 而不是 `run_coroutine_threadsafe`：**
- 回复操作（send/stream）有顺序要求，Queue 天然 FIFO
- 多轮对话层（chat_message 可能多次出现）需要保证顺序
- 流量控制：消费者可以统一做速率控制，不会并发的 send 打爆飞书 API

---

## 四、核心组件设计

### 4.1 FeishuConfig — 配置

```python
# server/lark_service/config.py

@dataclass
class FeishuConfig:
    app_id: str
    app_secret: str
    enabled: bool = True
    domain: str = "open.feishu.cn"
    transport: str = "ws"             # "ws" | "webhook"
    encrypt_key: str = ""            # webhook 模式需要
    verification_token: str = ""     # webhook 模式需要

    # 策略
    dm_enabled: bool = True
    group_enabled: bool = True
    require_mention: bool = True
    dedup_ttl_hours: int = 12

    # 占位回复
    placeholder_text: str = "思考中..."

    @classmethod
    def from_env(cls) -> "FeishuConfig":
        return cls(
            app_id=os.getenv("LARK_APP_ID", ""),
            app_secret=os.getenv("LARK_APP_SECRET", ""),
            ...
        )
```

### 4.2 ChannelManager — 生命周期

```python
# server/lark_service/channel_manager.py

class ChannelManager:
    """
    FeishuChannel 的完整生命周期管理。

    职责:
      - 根据配置创建 FeishuChannel
      - 注册事件处理器
      - 启动 / 停止
      - 健康检查与重连
    """

    def __init__(self, config: FeishuConfig):
        self._config = config
        self._channel: Optional[FeishuChannel] = None

    @property
    def channel(self) -> FeishuChannel:
        if self._channel is None:
            raise RuntimeError("Channel 未初始化")
        return self._channel

    @property
    def raw_client(self):
        """获取底层 OpenAPI Client，用于非 Channel 的 API 调用"""
        return self.channel.client

    async def start(self, event_handler: "EventHandler") -> None:
        """
        创建并启动 FeishuChannel。
        应在 FastAPI lifespan 启动阶段调用。
        """
        self._channel = FeishuChannel(
            app_id=self._config.app_id,
            app_secret=self._config.app_secret,
            domain=self._config.domain or None,
            transport=self._config.transport,
            encrypt_key=self._config.encrypt_key or None,
            verification_token=self._config.verification_token or None,
            policy=PolicyConfig(
                dm_policy="open" if self._config.dm_enabled else "disabled",
                group_policy="open" if self._config.group_enabled else "disabled",
                require_mention=self._config.require_mention,
            ),
            safety=SafetyConfig(
                dedup=DedupConfig(
                    ttl_seconds=self._config.dedup_ttl_hours * 3600
                ),
            ),
        )

        # 注册事件
        event_handler.bind(self._channel)

        # 启动
        await self._channel.connect_until_ready(timeout=30)

    async def stop(self) -> None:
        """停止 Channel 连接"""
        if self._channel:
            await self._channel.disconnect()
            self._channel = None

    async def health_check(self) -> bool:
        """检查连接是否存活"""
        # connect_until_ready 成功后 channel 应始终可用
        return self._channel is not None
```

### 4.3 EventHandler — 事件分发与调度

```python
# server/lark_service/event_handler.py

class EventHandler:
    """
    飞书事件 → 消息转换 → ReAct 循环 → 回复输出。

    这是整个 Adapter 的核心编排器。
    """

    def __init__(self, config: FeishuConfig):
        self._config = config
        self._converter = MessageConverter()
        self._channel: Optional[FeishuChannel] = None

    def bind(self, channel: FeishuChannel) -> None:
        """注册事件回调到 Channel"""
        self._channel = channel
        channel.on("message", self._handle_message)
        channel.on("cardAction", self._handle_card_action)
        channel.on("botAdded", self._handle_system_event)
        channel.on("botLeave", self._handle_system_event)
        channel.on("reaction", self._handle_system_event)
        channel.on("reject", self._handle_reject)
        channel.on("error", self._handle_error)

    # ── 消息处理（核心路径） ──

    async def _handle_message(self, msg: InboundMessage) -> None:
        # ① 转换 → session entry
        entry = self._converter.to_session_entry(msg)

        # ② 初始化回复适配器
        adapter = ReplyAdapter(self._channel, msg.chat_id, entry["meta"])

        # ③ 发占位消息
        await adapter.send_placeholder(self._config.placeholder_text)

        # ④ 创建跨线程队列
        queue: asyncio.Queue = asyncio.Queue()

        # ⑤ 启动消费者
        consumer = asyncio.create_task(
            adapter.consume(queue),
            name=f"feishu_reply_{msg.message_id}",
        )

        # ⑥ 线程池执行 ReAct
        loop = asyncio.get_event_loop()
        try:
            await loop.run_in_executor(
                None,
                self._run_react,
                entry["content"],
                queue,
                loop,
            )
        except Exception as e:
            await queue.put(("error", {"message": str(e)}))
        finally:
            await queue.put(("__sentinel__", {}))

        # 等待消费者完成
        await consumer

    def _run_react(
        self,
        user_input: str,
        queue: asyncio.Queue,
        loop: asyncio.AbstractEventLoop,
    ) -> None:
        """在线程池中执行（同步函数）"""
        from server.services.chat_service import streaming_react_loop

        def on_event(event_type: str, data: dict) -> None:
            try:
                loop.call_soon_threadsafe(queue.put_nowait, (event_type, data))
            except RuntimeError:
                pass  # 事件循环已关闭

        streaming_react_loop(
            user_input,
            on_event=on_event,
            channel="feishu",
        )

    # ── 卡片交互 ──

    async def _handle_card_action(self, event: CardActionEvent) -> None:
        entry = self._converter.card_action_to_entry(event)
        # 复用消息处理流程
        adapter = ReplyAdapter(self._channel, entry["meta"]["chat_id"], entry["meta"])
        queue: asyncio.Queue = asyncio.Queue()
        consumer = asyncio.create_task(adapter.consume(queue))
        # ...同上

    # ── 系统事件 ──

    async def _handle_system_event(self, event) -> None:
        entry = self._converter.system_event_to_entry(event)
        from loop import session_manage
        session_manage.update_current_session([entry], "feishu")
        # 系统事件不需要 LLM 回复，只记录

    # ── 异常事件 ──

    async def _handle_reject(self, event: RejectEvent) -> None:
        # SDK 策略/安全层丢弃的消息，打日志即可
        pass

    async def _handle_error(self, error: Exception) -> None:
        # 统一错误观测
        import traceback
        traceback.print_exc()
```

### 4.4 MessageConverter — 消息转换

```python
# server/lark_service/message_converter.py

class MessageConverter:
    """
    飞书消息 ↔ 标准 session entry。

    参见 doc/design/feishu-session-format.md 中的 content 拼接规则。
    """

    def to_session_entry(self, msg: InboundMessage) -> dict:
        """InboundMessage → {role, time, content, meta}"""
        ...

    def card_action_to_entry(self, event: CardActionEvent) -> dict:
        """CardActionEvent → {role, time, content, meta}"""
        ...

    def system_event_to_entry(self, event) -> dict:
        """系统事件 → {role, time, content, meta}"""
        ...

    # ── 内部 ──

    def _build_content(self, msg: InboundMessage) -> str:
        """拼接 LLM 可读的 content 字段"""
        ...

    def _build_meta(self, msg: InboundMessage) -> dict:
        """构造结构化 meta"""
        ...

    def _build_reply_context(self, msg: InboundMessage) -> Optional[dict]:
        """解析回复上下文（reply_to）"""
        ...
```

转换规则已在 [feishu-session-format.md](feishu-session-format.md) 中详细定义。

### 4.5 ReplyAdapter — 回复输出

```python
# server/lark_service/reply_adapter.py

class ReplyAdapter:
    """
    将 ReAct 事件流转为飞书消息发送。

    三种输出策略：
      Phase 1 (当前实现):
        - 收集所有 chat_message delta
        - 在 done 时一次性发送完整 markdown

      Phase 2 (后续升级):
        - 利用 CardKit 流式更新
        - 逐 token 发送，用户在飞书看到实时输出
    """

    def __init__(
        self,
        channel: FeishuChannel,
        chat_id: str,
        msg_meta: dict,
    ):
        self._channel = channel
        self._chat_id = chat_id
        self._msg_meta = msg_meta          # 用于 reply_to 定位
        self._accumulated_text = ""        # Phase 1 累积缓冲区
        self._result: Optional[SendResult] = None

    async def send_placeholder(self, text: str = "思考中...") -> None:
        """发送占位消息，告知用户正在处理"""
        self._placeholder = await self._channel.send(
            self._chat_id,
            {"text": text},
        )

    async def consume(self, queue: asyncio.Queue) -> None:
        """
        消费 ReAct 事件队列，转换为飞书消息发送。

        事件处理:
          chat_thought   → (内部日志，不发送给用户)
          chat_message   → 累积 delta 或流式更新
          chat_stop      → 完成发送
          action_start   → (日志)
          action_step    → (日志)
          action_tool_call   → [可选] 发送工具调用通知
          action_tool_result → [可选] 发送工具结果
          action_finished    → (日志)
          error          → 发送错误消息
          __sentinel__   → 退出，触发 finalize
        """
        while True:
            event_type, data = await queue.get()

            if event_type == "__sentinel__":
                await self._finalize()
                break

            handler = getattr(self, f"_on_{event_type}", None)
            if handler:
                await handler(data)

    # ── Phase 1: 累积 + 一次性发送 ──

    async def _on_chat_message(self, data: dict) -> None:
        self._accumulated_text += data.get("delta", "")

    async def _on_chat_stop(self, data: dict) -> None:
        pass  # 在 _finalize 中统一处理

    async def _on_action_tool_call(self, data: dict) -> None:
        # 可选：发送 "正在执行 {tool_name}..." 的状态更新
        pass

    async def _on_error(self, data: dict) -> None:
        await self._channel.send(
            self._chat_id,
            {"text": f"处理出错: {data.get('message', '未知错误')}"},
        )

    async def _finalize(self) -> None:
        """发送最终回复"""
        if self._accumulated_text:
            await self._channel.send(
                self._chat_id,
                {"markdown": self._accumulated_text},
                {"reply_to": self._msg_meta.get("message_id")},
            )
        elif self._placeholder:
            # 没有文本输出的情况下，更新占位消息
            await self._channel.edit_message(
                self._placeholder.message_id,
                {"text": "处理完成"},
            )

    # ── Phase 2 预留接口 (流式) ──

    async def _start_stream(self, initial_text: str) -> None:
        """创建 CardKit 流式卡片，后续逐 token 更新"""
        raise NotImplementedError("Phase 2")

    async def _stream_append(self, delta: str) -> None:
        """向流式卡片追加 token"""
        raise NotImplementedError("Phase 2")

    async def _finish_stream(self) -> None:
        """结束流式输出"""
        raise NotImplementedError("Phase 2")
```

**Phase 1 vs Phase 2 对比：**

| | Phase 1 (累积发送) | Phase 2 (流式输出) |
|---|---|---|
| 用户看到首字的延迟 | LLM 完全响应后 | LLM 返回第一个 token 后 |
| 实现复杂度 | 低 | 中（需 CardKit 预分配+sequence 管理） |
| 飞书体验 | "思考中..." → 完整回复一次性出现 | 逐字逐句实时显示（类 ChatGPT） |
| SDK 依赖 | `channel.send()` | `channel.stream()` + CardKit 底层 API |
| 开发建议 | 优先实现，验证连通性 | Phase 1 稳定后升级 |

---

## 五、与 FastAPI 的集成

### 5.1 Lifespan 中管理生命周期

```python
# server/main.py 修改

from contextlib import asynccontextmanager

def create_app() -> FastAPI:
    app = FastAPI(...)

    # ... CORS、路由注册 ...

    # 注册 lifespan
    @asynccontextmanager
    async def lifespan(app: FastAPI):
        await _startup_lark(app)
        try:
            yield
        finally:
            await _shutdown_lark(app)

    app.router.lifespan_context = lifespan
    return app


async def _startup_lark(app: FastAPI) -> None:
    """启动飞书渠道（如果配置了凭据）"""
    from server.lark_service.config import FeishuConfig
    from server.lark_service.channel_manager import ChannelManager
    from server.lark_service.event_handler import EventHandler

    config = FeishuConfig.from_env()
    if not config.app_id or not config.enabled:
        app.state.lark_manager = None
        return

    handler = EventHandler(config)
    manager = ChannelManager(config)
    await manager.start(handler)

    app.state.lark_manager = manager
    app.state.lark_handler = handler

async def _shutdown_lark(app: FastAPI) -> None:
    manager = getattr(app.state, "lark_manager", None)
    if manager:
        await manager.stop()
```

### 5.2 Webhook 模式路由（可选）

当 `transport="webhook"` 时，需要额外注册一个 HTTP 端点接收飞书的 POST 请求：

```python
# server/routers/lark.py（新增）

from fastapi import APIRouter, Request, Response

router = APIRouter(prefix="/lark", tags=["Lark"])

@router.post("/webhook")
async def lark_webhook(request: Request):
    manager = request.app.state.lark_manager
    if manager is None:
        return Response(status_code=503)

    status, body = await manager.channel.handle_webhook_request(
        headers=dict(request.headers),
        body=await request.body(),
    )
    return Response(status_code=status, content=body, media_type="application/json")
```

### 5.3 不修改的现有逻辑

```
server/routers/chat.py       → 零改动 ✅
server/services/chat_service.py → 零改动 ✅  (复用 streaming_react_loop)
server/routers/session.py    → 零改动 ✅
server/services/session_service.py → 零改动 ✅
server/schemas/              → 零改动 ✅
```

---

## 六、异常与容错

### 6.1 错误边界

```
层级                  异常处理策略
──────────────────────────────────────────────────
FeishuChannel         SDK 内置重连，触发 on("reconnecting") 和 on("reconnected")
   │                  on("error") → 打日志 + sentry
   │                  on("reject") → 静默（策略丢弃属正常行为）
   │
event_handler         每次 handle_* 包裹 try/except
   │                  handle_message 异常 → send 错误消息到飞书
   │                  handle_card_action 异常 → 忽略（卡片交互不应阻塞）
   │
ReAct 线程             线程内异常 → 推入 error 事件到队列
   │                  JSON 解析失败 → 已有 error 事件
   │
reply_adapter         send/stream 失败 → SendResult.success=False
                      已发送的消息不应重试（防止重复内容）
```

### 6.2 重连策略

FeishuChannel 的 `on("reconnecting")` 和 `on("reconnected")` 事件可注册回调用于观测：

```python
channel.on("reconnecting", lambda: print("[Lark] 重连中..."))
channel.on("reconnected", lambda: print("[Lark] 已重连"))
```

SDK 内置指数退避重连，不需要自己实现。

### 6.3 OpenAPI 限流

飞书 OpenAPI 有[频控策略](https://open.feishu.cn/document/ukTMukTMukTM/uQjN3QjL0YzN04CN2cDN)。对于个人助手场景（低频使用），正常使用不会触发。如果后续需要批量发送，可在 `ReplyAdapter` 中加 `asyncio.Semaphore` 限流。

---

## 七、配置项总览

```bash
# .env 飞书相关配置

# 飞书应用凭据（必填）
LARK_APP_ID=cli_xxxxxxxx
LARK_APP_SECRET=your_app_secret

# 飞书渠道开关
FEISHU_ENABLED=true                      # 默认 true（当凭据存在时）

# 域名（默认飞书，海外 Lark 用 open.larksuite.com）
FEISHU_DOMAIN=open.feishu.cn

# 传输方式: ws (WebSocket 长连接) / webhook (HTTP 回调)
FEISHU_TRANSPORT=ws

# Webhook 模式专属（transport=webhook 时需要）
LARK_ENCRYPT_KEY=
LARK_VERIFICATION_TOKEN=

# 策略
FEISHU_DM_ENABLED=true                   # 是否接收单聊消息
FEISHU_GROUP_ENABLED=true                # 是否接收群聊消息
FEISHU_REQUIRE_MENTION=true              # 群聊是否要求 @bot
FEISHU_DEDUP_TTL_HOURS=12                # 消息去重时间窗口（小时）

# 占位回复文本
FEISHU_PLACEHOLDER_TEXT=思考中...
```

---

## 八、文件变更清单

```
新增文件 (6 个):
  server/lark_service/__init__.py
  server/lark_service/config.py
  server/lark_service/channel_manager.py
  server/lark_service/event_handler.py
  server/lark_service/message_converter.py
  server/lark_service/reply_adapter.py

修改文件 (1 个):
  server/main.py                          ← 添加 lifespan，集成 ChannelManager

新增依赖 (1 个):
  pip install lark-channel-sdk

不受影响 (全部):
  agent/llm/llm.py
  agent/loop/react_loop.py
  agent/loop/session_manage.py
  agent/loop/prompt_assemble.py
  agent/loop/zvec_store.py
  agent/tool/base_tool.py
  agent/config/env_config.py
  server/services/chat_service.py
  server/services/session_service.py
  server/services/memory_service.py
  server/services/config_service.py
  server/services/tool_service.py
  server/routers/*
  server/schemas/*
  server/core/*
```

---

## 九、实施计划

| 阶段 | 内容 | 产出 | 依赖 |
|------|------|------|:--:|
| **P0: 准备** | 安装 `lark-channel-sdk`；配置飞书应用（创建应用、开权限、开事件订阅、发布） | 飞书应用就绪，能收到事件 | — |
| **P1: 连通性** | 实现 `config` + `channel_manager`，在 `main.py` lifespan 中启动 Channel，echo bot 验证 | 飞书消息 → echo 回复 | P0 |
| **P2: 核心接入** | 实现 `event_handler` + `message_converter` + `reply_adapter` (Phase 1)，接入 ReAct 循环 | 飞书消息 → LLM 处理 → 完整回复 | P1 |
| **P3: 会话记忆** | 对接 `session_manage`，飞书消息持久化到 `current/feishu.json`，压缩+ZVec 写入 | 飞书对话记忆正常运作 | P2 |
| **P4: 多模态** | `message_converter` 支持图片/文件/卡片交互 → 资源标记 | 非文本消息有合理占位提示 | P2 |
| **P5: 流式输出** | `reply_adapter` Phase 2 — CardKit 流式更新 | 飞书用户看到逐字实时输出 | P2 |
| **P6: 运维** | 日志、错误上报、连接监控 | 线上稳定运行 | P3 |
