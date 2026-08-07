// ============================================================
// ChatInput —— 消息输入区域组件
//
// 【React 概念】受控组件 vs 非受控组件
//
// 受控组件 (Controlled):
//   input 的值由 React state 管理。
//   每次输入都触发 setState → 组件重渲染 → input 显示新值。
//   对比 Vue 的 v-model（本质也是受控组件）。
//
//   ✅ 优点：React 完全控制输入值，可实时验证、格式化
//   ❌ 缺点：每次按键都重渲染（React 19 已大幅优化）
//
// 非受控组件 (Uncontrolled):
//   用 useRef 直接读取 DOM 值，React 不管理输入状态。
//   对比 Vue 的 ref + .value。
//
//   这里使用受控组件方式，演示 React 中最常见的表单处理模式。
// ============================================================

import React, { useState, useRef, useCallback, type KeyboardEvent } from "react";
import { Button, Input, Space } from "antd";
import type { TextAreaRef } from "antd/es/input/TextArea";
import { SendOutlined } from "@ant-design/icons";

const { TextArea } = Input;

interface ChatInputProps {
  /** 发送消息的回调 */
  onSend: (message: string) => void;
  /** 是否禁用输入（加载中时禁用） */
  disabled?: boolean;
}

const ChatInput: React.FC<ChatInputProps> = ({ onSend, disabled = false }) => {
  // ==========================================================
  //  useState —— 受控组件的核心
  //
  //  inputValue 就是"事实来源"。
  //  <TextArea value={inputValue} onChange={...} />
  //  不像 Vue 的 v-model，React 需要手动绑定 value + onChange。
  // ==========================================================
  const [inputValue, setInputValue] = useState("");

  // 输入框 ref —— 用于发送后重新聚焦
  // antd 的 TextArea ref 类型是 TextAreaRef（包含 focus、blur 等方法）
  const inputRef = useRef<TextAreaRef>(null);

  /**
   * 发送消息
   *
   * useCallback + 依赖数组 确保函数引用稳定。
   * 只有 onSend 或 inputValue 变化时才重新创建。
   *
   * 注意：这里把 inputValue 作为依赖，
   * 所以 Input 的 onChange 会导致此函数重新创建。
   * 但对 TextArea 这种原生组件影响不大。
   */
  const handleSend = useCallback(() => {
    const trimmed = inputValue.trim();
    if (!trimmed || disabled) return;

    onSend(trimmed);
    // 清空输入框（React 不可变更新）
    setInputValue("");
    // 发送后重新聚焦（提升体验）
    // setTimeout 等下一个事件循环再聚焦（等 React 完成更新）
    setTimeout(() => {
      inputRef.current?.focus();
    }, 0);
  }, [inputValue, disabled, onSend]);

  /**
   * 键盘事件处理 —— Enter 发送，Shift+Enter 换行
   *
   * React 合成事件 (SyntheticEvent)：
   *   事件处理函数接收的不是原生 DOM 事件，
   *   而是 React 包装的"合成事件"，跨浏览器一致。
   *
   * 对比 Vue:
   *   @keydown.enter="handleSend"  —— Vue 提供按键修饰符
   *   React 需要手动判断 e.key === "Enter"
   */
  const handleKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      // Enter 发送（Shift+Enter 换行）
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault(); // 阻止默认行为（换行）
        handleSend();
      }
    },
    [handleSend]
  );

  return (
    <div
      style={{
        padding: "12px 16px",
        borderTop: "1px solid #f0f0f0",
        background: "#fff",
      }}
    >
      <Space.Compact style={{ width: "100%", display: "flex" }}>
        <TextArea
          /**
           * ref 绑定 —— 获取 DOM 引用
           * 对比 Vue: <textarea ref="inputRef" v-model="inputValue" />
           *
           * React 中同时使用 ref 和 value 是完全正常的。
           */
          ref={inputRef}
          /**
           * 受控组件的三要素：
           * 1. value={state}       —— 值由 state 控制
           * 2. onChange={handler}  —— 更新 state
           * 3. state 变量           —— 事实来源
           *
           * 对比 Vue:
           *   v-model="inputValue" 一行搞定上面三步
           */
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          //        ↑ e.target.value 获取输入框当前值（类似 Vue 的 $event.target.value）
          onKeyDown={handleKeyDown}
          placeholder="输入消息，Enter 发送，Shift+Enter 换行"
          disabled={disabled}
          autoSize={{ minRows: 1, maxRows: 5 }}
          style={{ resize: "none" }}
        />
        <Button
          type="primary"
          icon={<SendOutlined />}
          onClick={handleSend}
          disabled={disabled || !inputValue.trim()}
          //                      ↑ 空消息时禁用发送按钮
          style={{ height: "auto", minHeight: 40 }}
        >
          发送
        </Button>
      </Space.Compact>
    </div>
  );
};

export default ChatInput;
