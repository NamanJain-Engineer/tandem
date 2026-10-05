import { describe, expect, it } from "vitest";
import { SearchAddon } from "@xterm/addon-search";
import { Terminal } from "@xterm/xterm";
import { tandemNight } from "@tandem/ui";
import { countLabel, HIGHLIGHT_LIMIT, searchOptions, validQuery, type FindResult } from "./find";

const write = (term: Terminal, data: string) => new Promise<void>((resolve) => term.write(data, resolve));

describe("Find in terminal (F-CD-08)", () => {
  it("F-CD-08.1 'needle' at lines 10, 500 and 900 reads 1 of 3, and next/previous cycle", async () => {
    const term = new Terminal({ cols: 80, rows: 24, scrollback: 2_000, allowProposedApi: true });
    term.open(document.createElement("div"));
    const search = new SearchAddon({ highlightLimit: HIGHLIGHT_LIMIT });
    term.loadAddon(search);
    let result: FindResult | null = null;
    search.onDidChangeResults((r) => (result = { index: r.resultIndex, count: r.resultCount }));
    const lines = Array.from({ length: 1_000 }, (_, i) => ([10, 500, 900].includes(i + 1) ? `line ${i + 1} needle` : `line ${i + 1}`));
    await write(term, lines.join("\r\n"));
    const opts = searchOptions({ caseSensitive: false, regex: false }, tandemNight);
    const flags = { caseSensitive: false, regex: false };

    search.findNext("needle", opts);
    expect(countLabel("needle", result, true)).toBe("1 of 3");
    expect(term.getSelectionPosition()?.start.y).toBe(9);
    search.findNext("needle", opts);
    expect(countLabel("needle", result, true)).toBe("2 of 3");
    search.findNext("needle", opts);
    expect(countLabel("needle", result, true)).toBe("3 of 3");
    search.findNext("needle", opts);
    expect(countLabel("needle", result, true)).toBe("1 of 3");
    search.findPrevious("needle", opts);
    expect(countLabel("needle", result, true)).toBe("3 of 3");
    expect(term.getSelectionPosition()?.start.y).toBe(899);

    // Case and regex toggles.
    search.findNext("NEEDLE", searchOptions({ caseSensitive: true, regex: false }, tandemNight));
    expect(countLabel("NEEDLE", result, true)).toBe("No results");
    search.findNext("line (10|500) n", searchOptions({ caseSensitive: false, regex: true }, tandemNight));
    expect(countLabel("x", result, true)).toBe("1 of 2");
    expect(validQuery("(", { ...flags, regex: true })).toBe(false);
    expect(validQuery("(", flags)).toBe(true);
    term.dispose();
  });

  it("labels: empty query, invalid pattern, capped counts", () => {
    expect(countLabel("", null, true)).toBe("");
    expect(countLabel("(", null, false)).toBe("Invalid pattern");
    expect(countLabel("a", { index: 4, count: HIGHLIGHT_LIMIT }, true)).toBe("5 of 1000+");
    expect(countLabel("a", { index: -1, count: 7 }, true)).toBe("7 results");
  });

  it("highlights use the theme's #RRGGBB find colours", () => {
    const d = searchOptions({ caseSensitive: false, regex: false }, tandemNight).decorations;
    expect(d?.matchBackground).toMatch(/^#[0-9a-f]{6}$/i);
    expect(d?.activeMatchBackground).toMatch(/^#[0-9a-f]{6}$/i);
  });
});
