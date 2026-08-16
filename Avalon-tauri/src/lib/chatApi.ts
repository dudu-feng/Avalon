// 后端 LLM 命令的接口封装
//
// 组件不直接 invoke，而是通过这里的语义化函数调用。
// Tauri 命令参数默认 camelCase，与 Rust 端 snake_case 自动映射。

import { invoke } from '@tauri-apps/api/core';
import type { LlmResponse } from '../types/chat';

export type ChatParams = {
  systemPrompt: string;
  userInput: string;
  chatHistory: string;
};

export type ActionParams = {
  userInput: string;
  actionTarget: string;
  actionHistory: string;
};

/** 对话层 LLM 调用 */
export async function llmChat(params: ChatParams): Promise<LlmResponse> {
  return invoke<LlmResponse>('llm_chat', params);
}

/** 动作层 LLM 调用 */
export async function llmAction(params: ActionParams): Promise<LlmResponse> {
  return invoke<LlmResponse>('llm_action', params);
}

/** 会话压缩 LLM 调用 */
export async function llmCompress(sessionData: string): Promise<LlmResponse> {
  return invoke<LlmResponse>('llm_compress', { sessionData });
}
