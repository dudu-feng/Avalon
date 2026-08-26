// 渠道对接命令的接口封装
//
// 注意：start / testConnection 读的都是「已保存到 Avalon-config.toml 的配置」，
// 不是设置页里正在编辑的草稿。改完配置必须先保存再调，否则用的还是旧值。

import { invoke } from '@tauri-apps/api/core';
import type { ChannelStatus } from '../types/config';

/** 启动飞书渠道。已在运行则先停后启，改完配置保存后直接调它即可生效 */
export async function startFeishu(): Promise<void> {
  return invoke<void>('feishu_start');
}

/** 停止飞书渠道 */
export async function stopFeishu(): Promise<void> {
  return invoke<void>('feishu_stop');
}

/** 查询飞书渠道当前状态 */
export async function getFeishuStatus(): Promise<ChannelStatus> {
  return invoke<ChannelStatus>('feishu_status');
}

/** 测试凭证：只做一次端点协商，不建立长连接，也不影响正在运行的渠道 */
export async function testFeishuConnection(): Promise<void> {
  return invoke<void>('feishu_test_connection');
}
