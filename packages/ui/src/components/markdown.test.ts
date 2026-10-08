import { describe, expect, it } from "vitest";
import { parseInline, parseMarkdown, safeHref, splitStable } from "./markdown";

describe("markdown (F-TH-12 MarkdownView)", () => {
  it("F-TH-12 headings, paragraphs, rules and fenced code", () => {
    const blocks = parseMarkdown("# Plan\n\nFirst line\nsecond line\n\n---\n\n```ts\nconst a = 1;\n\nconst b = 2;\n```\n");
    expect(blocks.map((b) => b.t)).toEqual(["heading", "para", "hr", "code"]);
    expect(blocks[3]).toEqual({ t: "code", lang: "ts", v: "const a = 1;\n\nconst b = 2;" });
    expect(blocks[1]).toEqual({ t: "para", c: [{ t: "text", v: "First line\nsecond line" }] });
  });

  it("F-TH-12 inline code, strong, emphasis, strikethrough and links", () => {
    expect(parseInline("Run `npm test` **now**, *then* ~~never~~ [docs](https://example.com/a)")).toEqual([
      { t: "text", v: "Run " },
      { t: "code", v: "npm test" },
      { t: "text", v: " " },
      { t: "strong", c: [{ t: "text", v: "now" }] },
      { t: "text", v: ", " },
      { t: "em", c: [{ t: "text", v: "then" }] },
      { t: "text", v: " " },
      { t: "del", c: [{ t: "text", v: "never" }] },
      { t: "text", v: " " },
      { t: "link", href: "https://example.com/a", c: [{ t: "text", v: "docs" }] },
    ]);
    // snake_case and lone markers stay text.
    expect(parseInline("call snake_case_name * 2")).toEqual([{ t: "text", v: "call snake_case_name * 2" }]);
    expect(parseInline("see https://vibezeo.com/docs.")).toEqual([
      { t: "text", v: "see " },
      { t: "link", href: "https://vibezeo.com/docs", c: [{ t: "text", v: "https://vibezeo.com/docs" }] },
      { t: "text", v: "." },
    ]);
  });

  it("F-TH-12 raw HTML is text and unsafe link schemes don't become links", () => {
    expect(parseInline("<script>alert(1)</script>")).toEqual([{ t: "text", v: "<script>alert(1)</script>" }]);
    expect(parseInline("[click](javascript:alert(1))")).toEqual([{ t: "text", v: "click" }, { t: "text", v: ")" }]);
    expect(safeHref("javascript:alert(1)")).toBeNull();
    expect(safeHref("file:///etc/passwd")).toBeNull();
    expect(safeHref("mailto:a@b.co")).toBe("mailto:a@b.co");
  });

  it("F-TH-12 lists with task items and nesting, and ordered lists", () => {
    const [list] = parseMarkdown("- [x] tests pass\n- [ ] update docs\n  - nested\n- plain");
    expect(list?.t).toBe("list");
    if (list?.t !== "list") return;
    expect(list.items.map((i) => i.checked)).toEqual([true, false, null]);
    expect(list.items[1]?.blocks.map((b) => b.t)).toEqual(["para", "list"]);
    const [ol] = parseMarkdown("3. three\n4. four");
    expect(ol).toMatchObject({ t: "list", ordered: true, start: 3 });
  });

  it("F-TH-12 tables with alignment, quotes", () => {
    const [table, quote] = parseMarkdown("| File | +/- |\n|:--|--:|\n| a.ts | +3 |\n| b\\|c | −1 |\n\n> note\n> more");
    expect(table).toMatchObject({ t: "table", align: ["left", "right"] });
    if (table?.t === "table") {
      expect(table.rows).toHaveLength(2);
      expect(table.rows[1]?.[0]).toEqual([{ t: "text", v: "b|c" }]);
    }
    expect(quote).toEqual({ t: "quote", blocks: [{ t: "para", c: [{ t: "text", v: "note\nmore" }] }] });
  });

  it("F-TH-12 an unclosed fence runs to the end (streaming), and only the tail re-parses", () => {
    expect(parseMarkdown("```sh\nnpm i")).toEqual([{ t: "code", lang: "sh", v: "npm i" }]);
    expect(splitStable("Para one.\n\n```\na\n\nb\n```\n\nTail being wri")).toEqual({
      stable: "Para one.\n\n```\na\n\nb\n```\n\n",
      tail: "Tail being wri",
    });
    expect(splitStable("no break yet")).toEqual({ stable: "", tail: "no break yet" });
  });
});
