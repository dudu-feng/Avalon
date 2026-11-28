// 桌面 run_id 生成与识别：这些轮次由命令流渲染，session-event 广播一律忽略。

/** 桌面 run_id：优先 crypto.randomUUID，旧 WebView 回退时间戳 + 自增。 */
let runSeq = 0;
export function newRunId(): string {
  try {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
      return `desktop_${crypto.randomUUID()}`;
    }
  } catch {
    // 非安全上下文等场景拿不到 randomUUID，走回退
  }
  return `desktop_${Date.now()}_${(runSeq += 1)}`;
}

/** 桌面自己发起的 run_id：这些轮次由命令流渲染，session-event 广播一律忽略。
 *  不能只靠 currentRunId 判断 —— send 收尾时会清空它，晚到的广播会被误当成后台轮次。 */
export const desktopRunIds = new Set<string>();
const DESKTOP_RUN_ID_LIMIT = 32;
export function rememberDesktopRun(runId: string) {
  desktopRunIds.add(runId);
  while (desktopRunIds.size > DESKTOP_RUN_ID_LIMIT) {
    const oldest = desktopRunIds.values().next().value;
    if (oldest === undefined) break;
    desktopRunIds.delete(oldest);
  }
}
