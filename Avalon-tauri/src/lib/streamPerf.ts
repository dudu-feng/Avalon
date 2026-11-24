// 流式渲染的开发期埋点：只在 `npm run dev` 下收集，production 构建里全部提前返回。
//
// 目的：判断剩余瓶颈到底在哪一层，避免凭感觉继续优化。
// - flush 耗时：一次合帧里事件路由 + 状态更新的主线程成本
// - delta → paint：从第一个未渲染的增量到达，到流式气泡完成绘制的时间
// - maxBacklog：单次合帧的事件数，持续偏高说明渲染仍跟不上 token 到达
//
// 每轮结束时在控制台打印一行 summary；需要更细的数据时再扩展成环形缓冲 / 面板。

const DEV = import.meta.env.DEV;

/** 供调用方跳过 performance.now() 等测量开销；production 下为 false */
export const streamPerfEnabled = DEV;

let active = false;
/** 轮次 token：防止上一轮的延迟收尾误伤刚开始的新一轮 */
let runToken = 0;
let runLabel = '';
let flushCount = 0;
let deltaCount = 0;
let maxBacklog = 0;
let warnedBacklog = false;
let flushDurations: number[] = [];
let paintLatencies: number[] = [];
/** 第一个尚未绘制到屏幕的增量到达时间；0 表示当前没有待绘制增量 */
let pendingSince = 0;

function percentile(values: number[], p: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[index];
}

export function beginStreamRun(label: string): number {
  if (!DEV) return 0;
  runToken += 1;
  active = true;
  runLabel = label;
  flushCount = 0;
  deltaCount = 0;
  maxBacklog = 0;
  warnedBacklog = false;
  flushDurations = [];
  paintLatencies = [];
  pendingSince = 0;
  return runToken;
}

export function noteDeltaReceived() {
  if (!DEV || !active) return;
  deltaCount += 1;
  if (pendingSince === 0) pendingSince = performance.now();
}

export function noteFlush(durationMs: number, eventCount: number) {
  if (!DEV || !active) return;
  flushCount += 1;
  flushDurations.push(durationMs);
  if (eventCount > maxBacklog) maxBacklog = eventCount;
  if (!warnedBacklog && eventCount > 60) {
    warnedBacklog = true;
    console.warn(
      `[stream-perf] 单次合帧 ${eventCount} 个事件：渲染可能仍跟不上 token 到达速度`,
    );
  }
}

/** 流式气泡完成一次绘制后调用（useEffect 内即绘制之后） */
export function notePainted() {
  if (!DEV || !active || pendingSince === 0) return;
  paintLatencies.push(performance.now() - pendingSince);
  pendingSince = 0;
}

/** 延迟到本轮最后一次绘制之后再收尾（调用方放进 setTimeout(0)） */
export function endStreamRun(token: number) {
  if (!DEV || !active || token !== runToken) return;
  active = false;
  if (flushCount === 0) return;
  const avgFlush =
    flushDurations.reduce((sum, value) => sum + value, 0) / Math.max(flushCount, 1);
  console.info('[stream-perf]', {
    run: runLabel,
    flushes: flushCount,
    deltas: deltaCount,
    avgFlushMs: Number(avgFlush.toFixed(2)),
    p95FlushMs: Number(percentile(flushDurations, 95).toFixed(2)),
    p95PaintMs: Number(percentile(paintLatencies, 95).toFixed(2)),
    maxBacklog,
  });
}
