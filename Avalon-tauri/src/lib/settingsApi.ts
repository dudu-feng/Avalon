// 后端配置管理命令的接口封装
//
// 组件不直接 invoke，而是通过这里的语义化函数调用。
// Tauri 命令参数默认 camelCase，与 Rust 端 snake_case 自动映射。

import { Channel, invoke } from '@tauri-apps/api/core';
import type { AppConfig, RebuildProgress, RebuildStats } from '../types/config';

/** 获取当前配置快照 */
export async function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>('get_config');
}

/** 保存配置（整体写回 Avalon-config.toml），返回校验警告列表 */
export async function saveConfig(config: AppConfig): Promise<string[]> {
  return invoke<string[]>('save_config', { newConfig: config });
}

/** 校验当前配置，返回警告列表 */
export async function validateConfig(): Promise<string[]> {
  return invoke<string[]>('validate_config');
}

/** 重建会话向量库（维护操作，设置页触发）；逐 session 处理经 onEvent 上报进度 */
export async function rebuildMemoryIndex(
  onEvent: (p: RebuildProgress) => void,
): Promise<RebuildStats> {
  const channel = new Channel<RebuildProgress>();
  channel.onmessage = onEvent;
  return invoke<RebuildStats>('rebuild_memory_index', { onEvent: channel });
}
