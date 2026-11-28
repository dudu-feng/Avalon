import { beforeEach, describe, expect, it } from "vitest";

import type { EngineEvent, TokenUsage } from "../../../types/chat";
import { applyEvent, createStore, type ChatStore } from "./useChatStore";

const USAGE: TokenUsage = {
  input_tokens: 1,
  output_tokens: 2,
  total_tokens: 3,
};

function done(message: string): EngineEvent {
  return {
    type: "done",
    result: {
      thought: "",
      message,
      tool_calls: [],
      usage: USAGE,
      model: "test-model",
    },
  };
}

describe("applyEvent 事件归约", () => {
  let store: ChatStore;

  beforeEach(() => {
    store = createStore();
  });

  it("没有流式槽位时增量事件被忽略", () => {
    const before = store.state;
    applyEvent(store, { type: "message_delta", delta: "x" }, "desktop");
    expect(store.state).toBe(before);
  });

  it("增量只更新 streaming，不触碰 messages", () => {
    applyEvent(store, { type: "round_start" }, "desktop");
    const messagesBefore = store.state.messages;

    applyEvent(store, { type: "message_delta", delta: "你" }, "desktop");
    applyEvent(store, { type: "thought_delta", delta: "想" }, "desktop");

    expect(store.state.messages).toBe(messagesBefore);
    expect(store.state.streaming?.content).toBe("你");
    expect(store.state.streaming?.thought).toBe("想");
  });

  it("round_start 封口上一轮并开新气泡", () => {
    applyEvent(store, { type: "round_start" }, "desktop");
    applyEvent(store, { type: "message_delta", delta: "第一轮" }, "desktop");
    applyEvent(store, { type: "round_start" }, "desktop");

    expect(store.state.messages).toHaveLength(1);
    const first = store.state.messages[0];
    expect(first.role).toBe("assistant");
    if (first.role === "assistant") {
      expect(first.content).toBe("第一轮");
    }
    expect(store.state.streaming?.content).toBe("");
  });

  it("tool_call 先落 assistant 再追加运行中工具卡片，tool_result 回填", () => {
    applyEvent(store, { type: "round_start" }, "desktop");
    applyEvent(store, { type: "thought_delta", delta: "先查文件" }, "desktop");
    applyEvent(
      store,
      { type: "tool_call", id: "call_1", tool_name: "read_file", arguments: { path: "a.txt" } },
      "desktop",
    );

    expect(store.state.streaming).toBeNull();
    expect(store.state.messages.map((m) => m.role)).toEqual(["assistant", "tool"]);
    const tool = store.state.messages[1];
    if (tool.role === "tool") {
      expect(tool.tool.status).toBe("running");
      expect(tool.tool.toolName).toBe("read_file");
      expect(tool.tool.arguments).toEqual({ path: "a.txt" });
    }

    applyEvent(
      store,
      { type: "tool_result", tool_name: "read_file", success: true, result: "内容" },
      "desktop",
    );
    const updated = store.state.messages[1];
    if (updated.role === "tool") {
      expect(updated.tool.status).toBe("success");
      expect(updated.tool.result).toBe("内容");
    }
  });

  it("空气泡的 tool_call 不生成空 assistant 消息", () => {
    applyEvent(store, { type: "round_start" }, "desktop");
    applyEvent(
      store,
      { type: "tool_call", id: "call_1", tool_name: "read_file", arguments: {} },
      "desktop",
    );

    expect(store.state.messages.map((m) => m.role)).toEqual(["tool"]);
    expect(store.state.streaming).toBeNull();
  });

  it("done 只提交当前轮正文，不使用跨轮累加的 result.message", () => {
    applyEvent(store, { type: "round_start" }, "desktop");
    applyEvent(store, { type: "message_delta", delta: "本轮流式" }, "desktop");
    applyEvent(store, done("跨轮累加正文"), "desktop");

    expect(store.state.messages).toHaveLength(1);
    const msg = store.state.messages[0];
    if (msg.role === "assistant") {
      expect(msg.content).toBe("本轮流式");
      expect(msg.status).toBe("done");
    }
  });

  it("没有流式事件时 done 用 result.message 兜底，空结果不落空气泡", () => {
    applyEvent(store, done("最终回复"), "desktop");
    expect(store.state.messages).toHaveLength(1);
    const msg = store.state.messages[0];
    if (msg.role === "assistant") {
      expect(msg.content).toBe("最终回复");
    }
    expect(store.state.streaming).toBeNull();

    const fresh = createStore();
    applyEvent(fresh, done(""), "desktop");
    expect(fresh.state.messages).toHaveLength(0);
  });

  it("error 事件把已生成内容落成错误消息", () => {
    applyEvent(store, { type: "round_start" }, "desktop");
    applyEvent(store, { type: "message_delta", delta: "半截" }, "desktop");
    applyEvent(store, { type: "error", code: 500, message: "连接中断" }, "desktop");

    expect(store.state.streaming).toBeNull();
    const msg = store.state.messages[0];
    if (msg.role === "assistant") {
      expect(msg.status).toBe("error");
      expect(msg.error).toBe("连接中断");
      expect(msg.content).toBe("半截");
    }
  });

  it("压缩提示先 running，compress_done 回填失败原因", () => {
    applyEvent(store, { type: "compress_start" }, "desktop");
    expect(store.state.messages[0]?.role).toBe("compress");

    applyEvent(store, { type: "compress_done", success: false, error: "模型不可用" }, "desktop");
    const msg = store.state.messages[0];
    if (msg.role === "compress") {
      expect(msg.status).toBe("error");
      expect(msg.error).toBe("模型不可用");
    }
  });
});
