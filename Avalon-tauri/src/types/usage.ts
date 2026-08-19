// 用量统计协议类型（对齐后端 usage::DailyUsageRow 展平结构）

/** 报表查询返回的一行：某天某个模型的累计用量 */
export interface DailyUsageRow {
  /** 本地日期，格式 "2026-08-20" */
  date: string;
  /** 生成这些用量的模型名 */
  model: string;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  reasoning_tokens: number;
  cached_tokens: number;
  /** 请求次数 */
  requests: number;
}
