// ============================================================
// useChat —— 聊天核心逻辑 Hook
//
// 【React vs Vue 对照】
// ┌─────────────────────┬──────────────────────────┐
// │ React Hook          │ Vue 3 Composition API    │
// ├─────────────────────┼──────────────────────────┤
// │ useState()          │ ref() / reactive()       │
// │ useEffect()         │ watch() / onMounted()    │
// │ useCallback()       │ 无需（函数默认稳定）       │
// │ useMemo()           │ computed()               │
// │ 自定义 Hook          │ Composables (useXxx)     │
// └─────────────────────┴──────────────────────────┘
//
// 【关键区别】
// 1. React 状态是"不可变"的 —— 你必须用 setState 替换整个值，
//    而不能像 Vue 那样 .value = xxx 直接修改。
// 2. React 组件每次状态变化都会"整体重新执行"，
//    而 Vue 是细粒度的依赖追踪，只更新变化的部分。
// 3. useCallback/useMemo 用于"缓存"函数/计算结果，
//    避免不必要的子组件重渲染 —— Vue 自动处理这些。
// ============================================================

import { useState, useCallback } from "react";
import type { ChatMessage } from "../types";
import { sendChatMessage } from "../services/tauriApi";

/**
 * useChat Hook
 *
 * 类似 Vue 中定义一个可复用的 Composable。
 * React 约定：自定义 Hook 必须以 "use" 开头。
 */
export function useChat() {
  // ==========================================================
  //  useState —— React 的响应式状态
  //
  //  const [值, 设置值函数] = useState(初始值)
  //
  //  对比 Vue 3:
  //    const messages = ref<ChatMessage[]>([])   // Vue
  //    const [messages, setMessages] = useState<ChatMessage[]>([])  // React
  //
  //  ⚠️ React 重要规则：永远不要直接修改 state！
  //    ❌ messages.push(newMsg)          // 不会触发重渲染
  //    ✅ setMessages([...messages, newMsg])  // 创建新数组替换
  // ==========================================================
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // ==========================================================
  //  useCallback —— 缓存函数引用
  //
  //  useCallback(fn, [依赖项]) 返回一个"记住"的函数。
  //  只有依赖项变化时才重新创建函数。
  //
  //  为什么需要？React 组件重新渲染时，普通函数会重新创建，
  //  导致子组件以为 props 变了而重新渲染。
  //
  //  对比 Vue: 不需要，因为 Vue 的响应式系统自动处理依赖追踪。
  // ==========================================================

  /**
   * 生成唯一 ID
   * 注意：这不是 React 概念，只是工具函数。
   * 放在组件外部（模块级别）不会在每次渲染时重新创建。
   */
  const generateId = (): string => {
    return `msg_${Date.now()}_${Math.random().toString(36).slice(2, 9)}`;
  };

  /**
   * 添加一条消息到列表
   *
   * useCallback 第二个参数 [] 表示"永不重新创建"（依赖为空）。
   * 对比 Vue: 直接定义函数即可，Vue 会自动处理缓存。
   */
  const addMessage = useCallback(
    (role: ChatMessage["role"], content: string, tokenUsage?: ChatMessage["tokenUsage"]) => {
      const newMsg: ChatMessage = {
        id: generateId(),
        role,
        content,
        timestamp: new Date().toISOString(),
        tokenUsage,
      };
      // React 不可变更新：创建新数组，而非 push
      setMessages((prev) => [...prev, newMsg]);
      return newMsg.id;
    },
    [] // 空依赖 = 这个函数永远不会变
  );

  /**
   * 发送消息 —— 核心方法
   *
   * 流程：
   * 1. 添加用户消息到列表
   * 2. 设置 loading = true（显示加载动画）
   * 3. 调用后端 API
   * 4. 添加 AI 回复到列表
   * 5. 设置 loading = false
   */
  const sendMessage = useCallback(
    async (userInput: string) => {
      if (!userInput.trim() || loading) return;

      // 添加用户消息
      addMessage("user", userInput.trim());
      setLoading(true);
      setError(null);

      try {
        // 构建聊天历史（JSON 字符串，传给后端）
        const chatHistory = JSON.stringify(
          messages.map((m) => ({
            role: m.role,
            content: m.content,
          }))
        );

        // 调用 Tauri 后端
        const response = await sendChatMessage({
          system_prompt:
            "你是一个智能助手，请用简洁准确的语言回答用户的问题。",
          user_input: userInput.trim(),
          chat_history: chatHistory,
        });

        // 添加 AI 回复
        addMessage("assistant", response.content, response.token_usage);
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : "发送消息失败";
        setError(errorMsg);
        // 添加错误消息到列表
        addMessage("system", `❌ 错误: ${errorMsg}`);
      } finally {
        setLoading(false);
      }
    },
    /**
     * 依赖数组 —— React 的重要概念
     *
     * useCallback 的第二个参数：告诉 React "什么时候需要重新创建这个函数"。
     * [messages, loading, addMessage] 表示这三个值变化时才重建。
     *
     * ⚠️ 如果遗漏依赖，函数内部会持有"过期"的 state 值（闭包陷阱）。
     * 这和 Vue 的响应式追踪完全不同 —— Vue 总能看到最新值。
     */
    [messages, loading, addMessage]
  );

  /**
   * 清空聊天记录
   */
  const clearMessages = useCallback(() => {
    setMessages([]);
    setError(null);
  }, []);

  // 返回暴露给组件的状态和方法
  // 对比 Vue: composable 的 return { ... }
  return {
    messages, // 消息列表
    loading, // 是否加载中
    error, // 错误信息
    sendMessage, // 发送消息方法
    clearMessages, // 清空方法
  } as const;
  //     ↑ as const 是 TS 特性，让返回值类型更精确
}
