// 手写 SVG 堆叠柱状图（两段：去强调的 base 在下，强调的 emphasis 在上）。
//
// 采用 emphasis 配色形式而非双色分类：base 用 --chart-muted（中性灰，承载「上下文」
// 语义），emphasis 用 --primary。同色系深浅两档实测过 —— brand-300 亮度越界且对比度
// 只有 1.87，brand-400/500 常视觉可辨识度仅 8.6（低于 15 底线），都不合格。
//
// 图元规格（逐条对齐可视化规范）：
// - 柱宽 ≤ 24px，不填满槽位，留白即呼吸感
// - 数据端 4px 圆角，基线端方角
// - 两段之间 2px 留白分隔（由下段顶部让出，保证总高仍然准确），而不是给柱子描边
// - 网格线 1px 实线（不用虚线），一档 off-surface 的灰
// - 文字一律用文字色，绝不用数据色；身份靠旁边的色块传达
// - 容器高度含 x 轴标签带，避免卡片里出现嵌套滚动条
// - tooltip 只是增强，精确读数由配套的表格视图承担

import { useEffect, useRef, useState } from 'react';
import styles from './StackedBarChart.module.css';

export interface StackedBarDatum {
  /** 唯一键 */
  key: string;
  /** x 轴标签 */
  label: string;
  /** 下层段（去强调） */
  base: number;
  /** 上层段（强调） */
  emphasis: number;
}

export interface StackedBarChartProps {
  data: StackedBarDatum[];
  /** 下层段名称，用于 tooltip */
  baseName: string;
  /** 上层段名称，用于 tooltip */
  emphasisName: string;
  /** 绘图区高度（不含 x 轴标签带） */
  height?: number;
  /** 数值格式化（y 轴刻度与 tooltip 共用） */
  formatValue?: (n: number) => string;
  /** x 轴标签抽稀：每隔几个标一个，1 为全标 */
  labelEvery?: number;
  /** 无数据时的提示文案 */
  emptyText?: string;
}

const GAP = 2;
const BAR_MAX_WIDTH = 24;
const PAD_LEFT = 52;
const PAD_RIGHT = 8;
const PAD_TOP = 10;
const AXIS_HEIGHT = 26;
const GRID_STEPS = 4;

/** 把最大值向上取整到「好看」的刻度上限（1/2/5 × 10^n） */
function niceMax(value: number): number {
  if (value <= 0) return 100;
  const exp = Math.floor(Math.log10(value));
  const base = 10 ** exp;
  const n = value / base;
  const step = n <= 1 ? 1 : n <= 2 ? 2 : n <= 5 ? 5 : 10;
  return step * base;
}

/** 顶部两角圆角、底部方角的矩形路径（数据端圆、基线端方） */
function topRoundedPath(x: number, y: number, w: number, h: number, r: number): string {
  if (h <= 0) return '';
  const rr = Math.min(r, h, w / 2);
  return [
    `M${x},${y + h}`,
    `L${x},${y + rr}`,
    `Q${x},${y} ${x + rr},${y}`,
    `L${x + w - rr},${y}`,
    `Q${x + w},${y} ${x + w},${y + rr}`,
    `L${x + w},${y + h}`,
    'Z',
  ].join(' ');
}

export function StackedBarChart({
  data,
  baseName,
  emphasisName,
  height = 200,
  formatValue = (n) => String(Math.round(n)),
  labelEvery = 1,
  emptyText = '暂无数据',
}: StackedBarChartProps) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [activeIndex, setActiveIndex] = useState<number | null>(null);

  // SVG 用真实像素坐标（而非 viewBox 缩放），否则文字会被拉伸变形
  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? 0;
      setWidth(w);
    });
    ro.observe(el);
    setWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  const totalHeight = height + AXIS_HEIGHT;
  const hasData = data.some((d) => d.base > 0 || d.emphasis > 0);

  const plotWidth = Math.max(0, width - PAD_LEFT - PAD_RIGHT);
  const plotHeight = height - PAD_TOP;
  const baseline = PAD_TOP + plotHeight;

  const maxTotal = Math.max(...data.map((d) => d.base + d.emphasis), 0);
  const scaleMax = niceMax(maxTotal);

  const slotWidth = data.length > 0 ? plotWidth / data.length : 0;
  const barWidth = Math.max(3, Math.min(BAR_MAX_WIDTH, slotWidth - 8));

  const toY = (v: number) => baseline - (v / scaleMax) * plotHeight;

  const active = activeIndex !== null ? data[activeIndex] : null;
  // tooltip 贴着柱心，靠近左右边缘时收拢，避免溢出卡片
  const activeCenter =
    activeIndex !== null ? PAD_LEFT + slotWidth * activeIndex + slotWidth / 2 : 0;
  const tooltipLeft = Math.min(Math.max(activeCenter, 80), Math.max(width - 80, 80));

  return (
    <div className={styles.wrap} ref={wrapRef}>
      {width > 0 && (
        <svg
          width={width}
          height={totalHeight}
          className={styles.svg}
          role="img"
          aria-label={`${emphasisName}与${baseName}的堆叠柱状图，共 ${data.length} 个时间点`}
        >
          {/* 网格线与 y 轴刻度：1px 实线，一档 off-surface */}
          {Array.from({ length: GRID_STEPS + 1 }, (_, i) => {
            const value = (scaleMax / GRID_STEPS) * i;
            const y = toY(value);
            return (
              <g key={`grid-${i}`}>
                <line
                  x1={PAD_LEFT}
                  y1={y}
                  x2={PAD_LEFT + plotWidth}
                  y2={y}
                  className={styles.grid}
                />
                <text x={PAD_LEFT - 8} y={y + 4} className={styles.axisText} textAnchor="end">
                  {formatValue(value)}
                </text>
              </g>
            );
          })}

          {hasData &&
            data.map((d, i) => {
              const slotX = PAD_LEFT + slotWidth * i;
              const barX = slotX + (slotWidth - barWidth) / 2;

              const baseH = (d.base / scaleMax) * plotHeight;
              const emphH = (d.emphasis / scaleMax) * plotHeight;
              const hasBoth = d.base > 0 && d.emphasis > 0;

              // 两段都有时，由下段顶部让出 2px 留白，总高度因此保持准确
              const baseDrawH = hasBoth ? Math.max(0, baseH - GAP) : baseH;
              const baseY = baseline - baseH + (hasBoth ? GAP : 0);
              const emphY = baseline - baseH - emphH;

              return (
                <g key={d.key}>
                  {/* 下段：顶上还有段时是方角，独自封顶时才是数据端（圆角） */}
                  {d.base > 0 &&
                    (hasBoth ? (
                      <rect
                        x={barX}
                        y={baseY}
                        width={barWidth}
                        height={baseDrawH}
                        className={styles.base}
                      />
                    ) : (
                      <path
                        d={topRoundedPath(barX, baseY, barWidth, baseDrawH, 4)}
                        className={styles.base}
                      />
                    ))}
                  {/* 上段：始终是数据端，顶部圆角 */}
                  {d.emphasis > 0 && (
                    <path
                      d={topRoundedPath(barX, emphY, barWidth, emphH, 4)}
                      className={styles.emphasis}
                    />
                  )}
                  {/* 命中区覆盖整个槽位并延伸到轴标签，保证够大好点 */}
                  <rect
                    x={slotX}
                    y={PAD_TOP}
                    width={slotWidth}
                    height={plotHeight + AXIS_HEIGHT}
                    className={styles.hit}
                    tabIndex={0}
                    role="button"
                    aria-label={`${d.label}：${emphasisName} ${formatValue(d.emphasis)}，${baseName} ${formatValue(d.base)}`}
                    onMouseEnter={() => setActiveIndex(i)}
                    onMouseLeave={() => setActiveIndex(null)}
                    onFocus={() => setActiveIndex(i)}
                    onBlur={() => setActiveIndex(null)}
                  />
                </g>
              );
            })}

          {/* x 轴标签：按 labelEvery 抽稀，避免拥挤重叠 */}
          {data.map((d, i) =>
            i % labelEvery === 0 ? (
              <text
                key={`label-${d.key}`}
                x={PAD_LEFT + slotWidth * i + slotWidth / 2}
                y={baseline + 18}
                className={styles.axisText}
                textAnchor="middle"
              >
                {d.label}
              </text>
            ) : null,
          )}

          {/* 基线：比网格线略实，锚住整个图 */}
          <line
            x1={PAD_LEFT}
            y1={baseline}
            x2={PAD_LEFT + plotWidth}
            y2={baseline}
            className={styles.axis}
          />
        </svg>
      )}

      {width > 0 && !hasData && <p className={styles.empty}>{emptyText}</p>}

      {active && (
        <div className={styles.tooltip} style={{ left: tooltipLeft }} role="status">
          <p className={styles.tooltipTitle}>{active.label}</p>
          <p className={styles.tooltipRow}>
            <span className={`${styles.swatch} ${styles.swatchEmphasis}`} aria-hidden="true" />
            {emphasisName}
            <span className={styles.tooltipValue}>{formatValue(active.emphasis)}</span>
          </p>
          <p className={styles.tooltipRow}>
            <span className={`${styles.swatch} ${styles.swatchBase}`} aria-hidden="true" />
            {baseName}
            <span className={styles.tooltipValue}>{formatValue(active.base)}</span>
          </p>
          <p className={`${styles.tooltipRow} ${styles.tooltipTotal}`}>
            合计
            <span className={styles.tooltipValue}>
              {formatValue(active.base + active.emphasis)}
            </span>
          </p>
        </div>
      )}
    </div>
  );
}
