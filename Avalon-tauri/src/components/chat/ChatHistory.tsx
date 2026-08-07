// ============================================================
// ChatHistory —— 消息列表组件（带自动滚动）
//
// 【React 概念】useRef 和 useEffect 配合实现 DOM 操作
//
// useRef —— 创建可变的引用，类似 Vue 的 ref() 获取 DOM 元素
//   Vue:  const divRef = ref<HTMLDivElement>()  → <div ref="divRef">
//   React: const divRef = useRef<HTMLDivElement>(null) → <div ref={divRef}>
//
// useEffect —— 处理副作用（DOM 更新、订阅、定时器等）
//   Vue: watchEffect(() => { ... }) 或 onMounted(() => { ... })
//
// 【关键区别】React 不自动追踪依赖！
//   Vue 的 watchEffect 自动追踪内部使用的响应式变量。
//   React 的 useEffect 需要你手动声明依赖数组 [messages]。
// ============================================================

import React, { useEffect, useRef } from "react";
import { Empty, Spin } from "antd";
import type { ChatMessage as ChatMessageType } from "../../types";
import ChatMessage from "./ChatMessage";

// Props 定义
interface ChatHistoryProps {
  /** 消息列表 */
  messages: ChatMessageType[];
  /** 是否正在加载（等待 AI 回复） */
  loading: boolean;
}

const ChatHistory: React.FC<ChatHistoryProps> = ({ messages, loading }) => {
  // ==========================================================
  //  useRef —— 创建 DOM 引用
  //
  //  useRef 创建一个 { current: 初始值 } 对象。
  //  修改 .current 不会触发重渲染（这点和 useState 不同）。
  //
  //  用途：
  //  1. 引用 DOM 元素：<div ref={myRef} />
  //  2. 保存可变值（不触发渲染）：timer、订阅等
  //
  //  对比 Vue:
  //    const divRef = ref<HTMLDivElement>()     // template ref
  //    const timer = shallowRef(null)            // 不需要响应式的变量
  // ==========================================================
  const bottomRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  /**
   * useEffect —— 消息列表变化时自动滚动到底部
   *
   * 第二个参数 [messages] 表示：
   * "当 messages 变化时，执行这个 effect"
   *
   * 对比 Vue 3:
   *   watch(messages, () => { scrollToBottom() })
   *   或者 watchEffect(() => { messages; scrollToBottom() })
   */
  useEffect(() => {
    scrollToBottom();
  }, [messages]);

  /** 滚动到列表底部 */
  const scrollToBottom = () => {
    // React 的 ref 通过 .current 访问，不像 Vue 的 .value
    bottomRef.current?.scrollIntoView({
      behavior: "smooth",
      block: "end",
    });
  };

  // 空状态 —— 还没有任何消息时显示
  if (messages.length === 0 && !loading) {
    return (
      <div
        style={{
          flex: 1,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <Empty
          description="开始一段新对话吧"
          image={Empty.PRESENTED_IMAGE_SIMPLE}
        />
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      //  ↑ React 的 ref 绑定，对比 Vue: ref="containerRef"
      style={{
        flex: 1,
        overflowY: "auto",
        padding: "8px 0",
        // 简单的滚动条样式
        scrollBehavior: "smooth",
      }}
    >
      {/* ========================================================
        JSX 中的 map 渲染列表
        对比 Vue: v-for="msg in messages" :key="msg.id"
        React: messages.map(msg => <Component key={msg.id} />)

        key 的作用：帮助 React 识别哪些元素变化了（diff 算法）
        和 Vue 的 :key 完全一样的原理！
      ======================================================== */}
      {messages.map((msg) => (
        <ChatMessage key={msg.id} message={msg} />
      ))}

      {/* 加载指示器 —— 等待 AI 回复时显示 */}
      {loading && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "12px 16px 12px 52px",
          }}
        >
          <Spin size="small" />
          <span style={{ color: "#999", fontSize: 13 }}>思考中...</span>
        </div>
      )}

      {/* 滚动锚点 —— 自动滚动到这个 div 即滚动到底部 */}
      <div ref={bottomRef} />
    </div>
  );
};

export default ChatHistory;
