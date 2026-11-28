import { describe, expect, it } from "vitest";

import { splitMarkdownBlocks } from "./markdownBlocks";

describe("splitMarkdownBlocks", () => {
  it("空文本返回空数组", () => {
    expect(splitMarkdownBlocks("")).toEqual([]);
  });

  it("按空行切分，最后一块标记未完成", () => {
    const blocks = splitMarkdownBlocks("第一段\n\n第二段");
    expect(blocks.map((b) => b.source)).toEqual(["第一段", "第二段"]);
    expect(blocks.map((b) => b.complete)).toEqual([true, false]);
  });

  it("代码围栏内部空行不切块，闭合后整块完成", () => {
    const source = "```js\nconst a = 1;\n\nconsole.log(a);\n```\n\n后续";
    const blocks = splitMarkdownBlocks(source);

    expect(blocks).toHaveLength(2);
    expect(blocks[0].source).toContain("```js");
    expect(blocks[0].source).toContain("console.log(a)");
    expect(blocks[0].complete).toBe(true);
    expect(blocks[1]).toEqual({ source: "后续", complete: false });
  });

  it("未闭合围栏保持为最后一块", () => {
    const blocks = splitMarkdownBlocks("开头\n\n```js\nconst a = 1;\n\nconst b = 2;");

    expect(blocks).toHaveLength(2);
    expect(blocks[1].source.startsWith("```js")).toBe(true);
    expect(blocks[1].complete).toBe(false);
  });

  it("波浪号围栏同样生效", () => {
    const blocks = splitMarkdownBlocks("~~~\na\n\nb\n~~~\n\n尾");

    expect(blocks).toHaveLength(2);
    expect(blocks[0].source).toBe("~~~\na\n\nb\n~~~");
    expect(blocks[0].complete).toBe(true);
  });

  it("多行与单行数学块都在边界处闭合", () => {
    const multi = splitMarkdownBlocks("$$\nx + y\n$$\n\n尾");
    expect(multi[0].source).toBe("$$\nx + y\n$$");
    expect(multi[0].complete).toBe(true);

    const single = splitMarkdownBlocks("$$x + y$$\n\n尾");
    expect(single[0].source).toBe("$$x + y$$");
    expect(single[0].complete).toBe(true);
  });

  it("被空行拆开的列表与引用块会重新合并", () => {
    const list = splitMarkdownBlocks("- 第一项\n\n- 第二项\n\n尾");
    expect(list[0].source).toBe("- 第一项\n\n- 第二项");
    expect(list[0].complete).toBe(true);

    const quote = splitMarkdownBlocks("> 引用一\n\n> 引用二\n\n尾");
    expect(quote[0].source).toBe("> 引用一\n\n> 引用二");
    expect(quote[0].complete).toBe(true);
  });

  it("列表与普通段落不跨空行合并", () => {
    const blocks = splitMarkdownBlocks("- 列表\n\n普通段落");
    expect(blocks.map((b) => b.source)).toEqual(["- 列表", "普通段落"]);
  });
});
