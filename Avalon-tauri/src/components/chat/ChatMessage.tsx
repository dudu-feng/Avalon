// ============================================================
// ChatMessage —— 单条消息气泡组件
//
// 【React 概念】Props 传递
//   父组件传值给子组件，类似 Vue 的 props。
//   React 中 props 是"只读"的 —— 子组件不能修改父组件传来的值。
//   这一点和 Vue 一致。
//
// 【React.memo】性能优化
//   类似 Vue 的 v-memo 或浅层比较。
//   如果 props 没变，React 会跳过这个组件的重渲染。
//   注意：和 Vue 不同，React 默认会在父组件更新时重渲染所有子组件。
// ============================================================

import React from "react";
import { Avatar, Typography, Space, theme } from "antd";
import { UserOutlined, RobotOutlined, InfoCircleOutlined } from "@ant-design/icons";
import type { ChatMessage as ChatMessageType } from "../../types";

const { Text, Paragraph } = Typography;

// ============================================================
//  Props 定义
//
//  对比 Vue:
//    interface Props { message: ChatMessageType }
//    const props = defineProps<Props>()
// ============================================================
interface ChatMessageProps {
  /** 消息数据 */
  message: ChatMessageType;
}

/**
 * 消息气泡组件
 *
 * React.memo 包裹函数组件：仅在 props 变化时重新渲染。
 * 对比 Vue: 默认行为类似（Vue 自动做依赖追踪），React 需要手动优化。
 */
const ChatMessage: React.FC<ChatMessageProps> = React.memo(({ message }) => {
  const { token: themeToken } = theme.useToken();

  // 根据角色决定样式和头像
  const isUser = message.role === "user";
  const isAssistant = message.role === "assistant";
  const isSystem = message.role === "system";

  /**
   * React 中根据条件返回不同 JSX —— 三种写法：
   * 1. 三元: {isUser ? <A /> : <B />}
   * 2. &&短路: {isUser && <A />}  （条件为 true 才渲染）
   * 3. 函数早返回: if (x) return <A />
   *
   * 对比 Vue: v-if / v-else / v-show
   *   - React 没有 v-show，条件渲染就是 JS 的条件语句
   *   - &&短路 ≈ v-if（不渲染 vs display:none）
   */

  return (
    <div
      style={{
        display: "flex",
        flexDirection: isUser ? "row-reverse" : "row",
        //              ↑ JS 三元表达式，对比 Vue template: :class="isUser ? 'reverse' : ''"
        padding: "12px 16px",
        gap: 12,
        maxWidth: "100%",
      }}
    >
      {/* 头像 —— 根据角色显示不同图标 */}
      <Avatar
        size={36}
        style={{
          backgroundColor: isUser
            ? themeToken.colorPrimary
            : isSystem
              ? themeToken.colorWarning
              : themeToken.colorSuccess,
          flexShrink: 0,
        }}
        icon={
          isUser ? (
            <UserOutlined />
          ) : isAssistant ? (
            <RobotOutlined />
          ) : (
            <InfoCircleOutlined />
          )
        }
      />

      {/* 消息内容 */}
      <div
        style={{
          maxWidth: "70%",
          display: "flex",
          flexDirection: "column",
          alignItems: isUser ? "flex-end" : "flex-start",
        }}
      >
        {/* 发送者标签 + 时间 */}
        <Space size={8} style={{ marginBottom: 4 }}>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {isUser ? "我" : isAssistant ? "Avalon" : "系统"}
          </Text>
          <Text type="secondary" style={{ fontSize: 11 }}>
            {formatTime(message.timestamp)}
          </Text>
          {/* Token 用量显示（仅 AI 消息） */}
          {message.tokenUsage && (
            <Text type="secondary" style={{ fontSize: 11 }}>
              🎯 {message.tokenUsage.total_tokens} tokens
            </Text>
          )}
        </Space>

        {/* 消息气泡 */}
        <div
          style={{
            padding: "10px 14px",
            borderRadius: 12,
            backgroundColor: isUser
              ? themeToken.colorPrimary
              : isSystem
                ? themeToken.colorWarningBg
                : themeToken.colorBgElevated,
            color: isUser ? themeToken.colorWhite : undefined,
            border: isUser
              ? "none"
              : `1px solid ${themeToken.colorBorderSecondary}`,
            wordBreak: "break-word",
          }}
        >
          <Paragraph
            style={{
              margin: 0,
              whiteSpace: "pre-wrap", // 保留换行
              color: "inherit",
            }}
          >
            {message.content}
          </Paragraph>
        </div>
      </div>
    </div>
  );
});

// ============================================================
//  组件显示名称（调试用，React DevTools 中显示）
// ============================================================
ChatMessage.displayName = "ChatMessage";

export default ChatMessage;

// ============================================================
//  工具函数 —— 格式化时间
//  放在组件外部（模块级别），不会在每次渲染时重新创建。
//  对比 Vue: 可以直接放在 <script setup> 中，Vite 会做 tree-shaking。
// ============================================================
function formatTime(isoString: string): string {
  const date = new Date(isoString);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMins = Math.floor(diffMs / 60000);

  if (diffMins < 1) return "刚刚";
  if (diffMins < 60) return `${diffMins}分钟前`;

  const hours = date.getHours().toString().padStart(2, "0");
  const mins = date.getMinutes().toString().padStart(2, "0");
  return `${hours}:${mins}`;
}
