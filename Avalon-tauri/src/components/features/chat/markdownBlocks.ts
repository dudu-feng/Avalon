// 流式 Markdown 的块划分：把「已经不会再变化的前缀」与「仍在增长的尾部」分开。
//
// 为什么需要：流式输出时每个 flush 都会拿整条正文重新做一遍 Markdown 解析。正文越长，
// 单次解析越慢，最终渲染速度跟不上 token 到达速度。按块切开后，已完成块的内容不再变化，
// 交给 memo 组件后只解析一次；每次 flush 只需重新解析最后一块（通常是正在增长的段落
// 或未闭合的代码块）。
//
// 划分保持保守：只在空行、代码围栏、$$ 数学块边界处切分。切分只影响流式期间的中间渲染，
// 消息完成后仍按整条正文一次性渲染，因此语义差异只可能短暂出现在生成过程中。

/** 一块可独立渲染的 Markdown 片段。 */
export interface MarkdownBlock {
  source: string;
  /** 已完成：后续增量不会改变它，可永久缓存解析结果 */
  complete: boolean;
}

/** 代码围栏起始行：三个及以上反引号或波浪号，允许缩进与 info string */
const FENCE_OPEN = /^[ \t]*(`{3,}|~{3,})/;

/** 闭合围栏：只有标记与可选空白，不允许 info string（兼容 CRLF 残留的 \r） */
const FENCE_CLOSE = /^[ \t]*(`{3,}|~{3,})[ \t\r]*$/;

/** $$ 数学块起始（含单行自闭合形式） */
const MATH_FENCE = /^[ \t]*\$\$/;

/** 列表项 / 引用块：跨空行合并时用于保持原有语义 */
const LIST_ITEM = /^[ \t]*(?:[-*+]|\d+[.)])[ \t]+/;
const BLOCKQUOTE = /^[ \t]*>/;

type Mode = 'normal' | 'fence' | 'math';

/** 取文本中第一个 / 最后一个非空行 */
function firstNonEmptyLine(text: string): string {
  for (const line of text.split('\n')) {
    if (line.trim()) return line;
  }
  return '';
}

function lastNonEmptyLine(text: string): string {
  const lines = text.split('\n');
  for (let i = lines.length - 1; i >= 0; i--) {
    if (lines[i].trim()) return lines[i];
  }
  return '';
}

/** 相邻两块是否仍属于同一个列表 / 引用块：空行切块后重新拼回，避免流式时列表被拆成两截 */
function continuesSameBlock(prev: string, next: string): boolean {
  const prevLast = lastNonEmptyLine(prev);
  const nextFirst = firstNonEmptyLine(next);
  if (!prevLast || !nextFirst) return false;
  if (LIST_ITEM.test(prevLast) && LIST_ITEM.test(nextFirst)) return true;
  return BLOCKQUOTE.test(prevLast) && BLOCKQUOTE.test(nextFirst);
}

/**
 * 把 Markdown 源文本切成若干块。
 *
 * - 空行结束当前普通块（连续空行只当作分隔）
 * - 代码围栏内部的空行不切块；闭合后整块结束
 * - `$$` 数学块同理；单行 `$$...$$` 直接视为自闭合
 * - 末尾未闭合的块保留为最后一块（流式期间会随增量继续增长）
 * - 跨空行的同一列表 / 引用块会重新合并
 */
export function splitMarkdownBlocks(source: string): MarkdownBlock[] {
  if (!source) return [];

  const lines = source.split('\n');
  const chunks: string[] = [];
  let current: string[] = [];
  let mode: Mode = 'normal';
  let fenceChar = '';
  let fenceLen = 0;

  const flush = () => {
    if (current.length === 0) return;
    chunks.push(current.join('\n'));
    current = [];
  };

  for (const line of lines) {
    if (mode === 'fence') {
      current.push(line);
      const close = FENCE_CLOSE.exec(line);
      if (close && close[1][0] === fenceChar && close[1].length >= fenceLen) {
        mode = 'normal';
        flush();
      }
      continue;
    }

    if (mode === 'math') {
      current.push(line);
      if (MATH_FENCE.test(line)) {
        mode = 'normal';
        flush();
      }
      continue;
    }

    const fence = FENCE_OPEN.exec(line);
    if (fence) {
      flush();
      current.push(line);
      mode = 'fence';
      fenceChar = fence[1][0];
      fenceLen = fence[1].length;
      continue;
    }

    if (MATH_FENCE.test(line)) {
      flush();
      current.push(line);
      const trimmed = line.trim();
      // 单行 $$...$$：起止同在一行，直接闭合；否则进入数学块模式
      if (trimmed.length > 4 && trimmed.endsWith('$$')) {
        flush();
      } else {
        mode = 'math';
      }
      continue;
    }

    if (line.trim() === '') {
      flush();
      continue;
    }

    current.push(line);
  }

  flush();

  // 合并被空行拆开的同一列表 / 引用块（保留原有中间空行，交给 Markdown 解析为松散列表）
  const merged: string[] = [];
  for (const chunk of chunks) {
    const prev = merged[merged.length - 1];
    if (prev !== undefined && continuesSameBlock(prev, chunk)) {
      merged[merged.length - 1] = `${prev}\n\n${chunk}`;
    } else {
      merged.push(chunk);
    }
  }

  // 除最后一块外都已确定不会变化；最后一块可能是仍在增长的段落 / 未闭合围栏
  return merged.map((chunk, index) => ({
    source: chunk,
    complete: index < merged.length - 1,
  }));
}
