import { describe, expect, it } from "vitest";

import type { DailyUsageRow } from "../../../types/usage";
import {
  aggregateByDate,
  aggregateByModel,
  buildDateRange,
  computeDashboard,
  emptySnapshot,
  formatCompact,
  formatFull,
  formatPercent,
  formatShortDate,
  localDate,
  makeKpi,
} from "./dashboardData";

function row(
  partial: Partial<DailyUsageRow> & Pick<DailyUsageRow, "date" | "model">,
): DailyUsageRow {
  return {
    input_tokens: 0,
    output_tokens: 0,
    total_tokens: 0,
    reasoning_tokens: 0,
    cached_tokens: 0,
    requests: 0,
    ...partial,
  };
}

describe("日期轴", () => {
  it("按本地日期升序生成，最后一天是今天", () => {
    const axis = buildDateRange(3, 0);
    expect(axis).toHaveLength(3);
    expect(axis[2]).toBe(localDate(new Date()));
  });

  it("offset 让窗口整体前移，且能跨月归一化", () => {
    const today = new Date();
    // range=3、offset=3：上一窗口的最后一天是「今天 - 3」
    const boundary = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 3);
    const previous = buildDateRange(3, 3);
    expect(previous[2]).toBe(localDate(boundary));

    // 两个窗口首尾不重叠
    const current = buildDateRange(3, 0);
    expect(previous[2] < current[0]).toBe(true);
  });
});

describe("聚合", () => {
  it("同一天多个模型先按天合并", () => {
    const agg = aggregateByDate([
      row({ date: "2026-08-20", model: "a", input_tokens: 10, total_tokens: 10, requests: 1 }),
      row({ date: "2026-08-20", model: "b", input_tokens: 5, total_tokens: 5, requests: 2 }),
    ]);

    expect(agg.get("2026-08-20")).toEqual({
      input: 15,
      output: 0,
      total: 15,
      cached: 0,
      requests: 3,
    });
  });

  it("模型排行只统计当前窗口，并按总量降序", () => {
    const agg = aggregateByModel(
      [
        row({ date: "2026-08-20", model: "a", total_tokens: 10, requests: 1 }),
        row({ date: "2026-08-20", model: "b", total_tokens: 30, requests: 2 }),
        row({ date: "2026-08-01", model: "a", total_tokens: 999, requests: 9 }),
      ],
      new Set(["2026-08-20"]),
    );

    expect(agg).toEqual([
      { model: "b", totalTokens: 30, requests: 2 },
      { model: "a", totalTokens: 10, requests: 1 },
    ]);
  });
});

describe("KPI", () => {
  it("正常环比", () => {
    expect(makeKpi(10, 4)).toEqual({ current: 10, previous: 4, delta: 6, deltaRatio: 1.5 });
  });

  it("上期为 0 或缺失时不产生 Infinity", () => {
    expect(makeKpi(10, 0)).toEqual({ current: 10, previous: 0, delta: 10, deltaRatio: null });
    expect(makeKpi(null, 4)).toEqual({
      current: null,
      previous: 4,
      delta: null,
      deltaRatio: null,
    });
  });
});

describe("computeDashboard", () => {
  it("按当前窗口汇总，并用上一窗口计算环比", () => {
    const currentAxis = buildDateRange(7, 0);
    const previousAxis = buildDateRange(7, 7);
    const today = currentAxis[currentAxis.length - 1];
    const lastPrevious = previousAxis[previousAxis.length - 1];

    const snap = computeDashboard({
      rows: [
        row({
          date: today,
          model: "a",
          input_tokens: 100,
          output_tokens: 20,
          total_tokens: 120,
          cached_tokens: 30,
          requests: 2,
        }),
        row({
          date: today,
          model: "b",
          input_tokens: 50,
          output_tokens: 10,
          total_tokens: 60,
          requests: 1,
        }),
        row({
          date: lastPrevious,
          model: "a",
          input_tokens: 10,
          total_tokens: 10,
          requests: 1,
        }),
        // 窗口外数据必须被忽略
        row({ date: "1999-01-01", model: "old", total_tokens: 99999, requests: 999 }),
      ],
      sessions: [],
      contextUsage: { used_tokens: 50, threshold: 200 },
      config: null,
      tasks: [],
      rangeDays: 7,
    });

    expect(snap.kpis.totalTokens).toEqual({
      current: 180,
      previous: 10,
      delta: 170,
      deltaRatio: 17,
    });
    expect(snap.kpis.requests.current).toBe(3);
    expect(snap.kpis.cacheHitRate.current).toBeCloseTo(30 / 150);
    expect(snap.kpis.avgTokensPerRequest.current).toBe(60);

    expect(snap.trends).toHaveLength(7);
    expect(snap.trends[6].totalTokens).toBe(180);
    expect(snap.trends[0].totalTokens).toBe(0);

    expect(snap.modelRanking).toEqual([
      { model: "a", totalTokens: 120, requests: 2 },
      { model: "b", totalTokens: 60, requests: 1 },
    ]);
    expect(snap.runStatus.contextRatio).toBe(0.25);
  });

  it("空数据快照不产生负数或 NaN", () => {
    const snap = emptySnapshot(30);
    expect(snap.trends).toHaveLength(30);
    expect(snap.kpis.totalTokens).toEqual({
      current: 0,
      previous: 0,
      delta: 0,
      deltaRatio: null,
    });
    expect(snap.kpis.cacheHitRate.current).toBeNull();
    expect(snap.kpis.avgTokensPerRequest.current).toBeNull();
    expect(snap.runStatus.contextRatio).toBeNull();
    expect(snap.runStatus.sessionCount).toBe(0);
  });
});

describe("展示格式化", () => {
  it("紧凑数字按量级切换", () => {
    expect(formatCompact(null)).toBe("—");
    expect(formatCompact(1284)).toBe("1,284");
    expect(formatCompact(9999)).toBe("9,999");
    expect(formatCompact(10000)).toBe("10.0K");
    expect(formatCompact(4_200_000)).toBe("4.2M");
  });

  it("完整千分位与百分比", () => {
    expect(formatFull(1234567.8)).toBe("1,234,568");
    expect(formatFull(Number.NaN)).toBe("—");
    expect(formatPercent(0.612)).toBe("61.2%");
    expect(formatPercent(0.612, 0)).toBe("61%");
    expect(formatPercent(Number.POSITIVE_INFINITY)).toBe("—");
  });

  it("日期短格式遇到异常输入原样返回", () => {
    expect(formatShortDate("2026-08-20")).toBe("8/20");
    expect(formatShortDate("bad")).toBe("bad");
  });
});
