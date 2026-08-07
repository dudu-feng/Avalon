// ============================================================
// Tauri API 服务层 —— 封装前端与 Rust 后端的通信
//
// React 中没有像 Vue 那样的 "api/" 目录约定，
// 但分层思想完全一致：把数据获取逻辑从组件中抽离。
//
// invoke() 是 Tauri 的 IPC 调用方式，类似于：
//   - Vue 中的 axios.get/post
//   - 浏览器中的 fetch()
// 它通过 IPC 桥调用 Rust 端的 #[tauri::command] 函数。
// ============================================================

import { invoke } from "@tauri-apps/api/core";
import type { AppConfig, ChatParams, LlmResponse } from "../types";

// ============================================================
//  系统初始化
// ============================================================

/**
 * 应用初始化 —— 前端启动时调用，获取配置校验结果
 *
 * 返回警告列表，空数组表示一切正常。
 * 前端据此判断是否需要引导用户配置 API Key。
 */
export async function initApp(): Promise<string[]> {
  return invoke("init_app");
}

// ============================================================
//  配置管理
// ============================================================

/** 获取当前应用配置 */
export async function getConfig(): Promise<AppConfig> {
  return invoke("get_config");
}

/** 保存应用配置（修改后调用） */
export async function saveConfig(config: AppConfig): Promise<void> {
  return invoke("save_config", { newConfig: config });
}

/** 校验配置完整性，返回警告列表 */
export async function validateConfig(): Promise<string[]> {
  return invoke("validate_config");
}

/** 获取 .env 配置文件路径 */
export async function getConfigPath(): Promise<string> {
  return invoke("get_config_path");
}

// ============================================================
//  LLM 调用
// ============================================================

/**
 * 发送聊天消息 —— 核心 API
 *
 * @param params - 聊天参数（系统提示词 + 用户输入 + 历史记录）
 * @returns LLM 的回复内容和 token 用量
 */
export async function sendChatMessage(params: ChatParams): Promise<LlmResponse> {
  return invoke("llm_chat", { params });
}

/**
 * 会话压缩 —— 用于超长对话的自动摘要
 *
 * @param sessionData - 需要压缩的会话数据（JSON 字符串）
 */
export async function compressSession(sessionData: string): Promise<LlmResponse> {
  return invoke("llm_compress", { sessionData });
}
