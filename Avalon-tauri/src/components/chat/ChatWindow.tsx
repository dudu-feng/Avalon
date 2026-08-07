// ============================================================
// ChatWindow —— 聊天窗口容器组件
//
// 组合 ChatHistory + ChatInput，通过 useChat Hook 管理状态。
//
// 【React 概念】状态提升 (Lifting State Up)
//   当多个子组件需要共享同一份数据时，把 state 放到它们的
//   最近公共父组件中，然后通过 props 向下传递。
//
//   这里 useChat 在 ChatWindow 中调用，
//   messages/loading 通过 props 传给 ChatHistory，
//   sendMessage 通过 props 传给 ChatInput。
//
//   对比 Vue: 同样的"状态提升"思路，
//   不过 Vue 也可以用 provide/inject 跨层级传递。
// ============================================================

import React, { useCallback } from "react";
import { Alert } from "antd";
import ChatHistory from "./ChatHistory";
import ChatInput from "./ChatInput";
import { useChat } from "../../hooks/useChat";

const ChatWindow: React.FC = () => {
  // ==========================================================
  //  使用自定义 Hook 获取聊天状态和方法
  //
  //  和 Vue 中使用 composable 完全一样：
  //    const { messages, sendMessage } = useChat()
  // ==========================================================
  const { messages, loading, error, sendMessage, clearMessages } = useChat();

  /**
   * 包装 sendMessage，添加额外的逻辑
   *
   * 这里 onSend 需要一个 (message: string) => void 的函数，
   * sendMessage 的签名刚好匹配，但通过 useCallback 确保引用稳定。
   */
  const handleSend = useCallback(
    (content: string) => {
      sendMessage(content);
    },
    [sendMessage]
  );

  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
      }}
    >
      {/* 错误提示 —— 仅在 error 存在时渲染 */}
      {error && (
        <Alert
          message={error}
          type="error"
          closable
          onClose={clearMessages}
          style={{ margin: "8px 16px 0" }}
        />
      )}

      {/* 消息列表 —— 自动滚动 */}
      <ChatHistory messages={messages} loading={loading} />

      {/* 输入区域 —— 底部固定 */}
      <ChatInput onSend={handleSend} disabled={loading} />
    </div>
  );
};

export default ChatWindow;
