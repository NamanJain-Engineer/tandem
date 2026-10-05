import { describe, expect, it, vi } from "vitest";

vi.mock("@xterm/xterm", () => ({ Terminal: class {} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class {} }));
const { quietQueries } = await import("./TerminalView");

type Csi = { id: { prefix?: string; final: string }; fn: (params: (number | number[])[]) => boolean };
type Osc = { code: number; fn: (data: string) => boolean };

function fakeParser() {
  const csi: Csi[] = [];
  const osc: Osc[] = [];
  const parser = {
    registerCsiHandler: (id: Csi["id"], fn: Csi["fn"]) => (
      csi.push({ id, fn }),
      { dispose: () => undefined }
    ),
    registerOscHandler: (code: number, fn: Osc["fn"]) => (
      osc.push({ code, fn }),
      { dispose: () => undefined }
    ),
  };
  quietQueries({ parser } as never);
  const takes = (prefix: string | undefined, final: string, params: number[] = []) =>
    csi.filter((h) => h.id.prefix === prefix && h.id.final === final).some((h) => h.fn(params));
  const takesOsc = (code: number, data: string) => osc.filter((h) => h.code === code).some((h) => h.fn(data));
  return { takes, takesOsc };
}

describe("terminal queries are answered once, by Rust (F-CD-03 AC5)", () => {
  it("F-CD-03 xterm leaves DA1, DA2, DSR, CPR, XTVERSION and colour queries to the core", () => {
    const { takes, takesOsc } = fakeParser();
    expect(takes(undefined, "c")).toBe(true);
    expect(takes(">", "c")).toBe(true);
    expect(takes(undefined, "n", [5])).toBe(true);
    expect(takes(undefined, "n", [6])).toBe(true);
    expect(takes(">", "q")).toBe(true);
    for (const code of [10, 11, 12]) expect(takesOsc(code, "?")).toBe(true);
  });

  it("F-CD-03 other sequences still reach xterm", () => {
    const { takes, takesOsc } = fakeParser();
    expect(takes(undefined, "n", [7])).toBe(false);
    expect(takesOsc(11, "#ffffff")).toBe(false);
    expect(takesOsc(0, "?")).toBe(false);
  });
});
