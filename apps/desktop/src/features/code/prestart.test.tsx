import { render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { setInvokeBackend } from "../../ipc/invoke";
import { guessSize, rememberFit } from "./prestart";

// F-CD-03 AC2: six agents' panes show within 300 ms of their tab. Views open
// one per frame (mountQueue), so a view waiting its turn starts its pane's
// process at once, at the grid the view that just opened fit, and attaches
// to it when it opens.

/** Anything called, read or disposed: a callable stand-in (as parkRelease.test). */
function stub(): unknown {
  return new Proxy(() => undefined, {
    get: (_t, p) => (p === "then" ? undefined : p === Symbol.toPrimitive ? () => 0 : stub()),
    apply: () => stub(),
  });
}

class FakeTerminal {
  cols = 80;
  rows = 24;
  textarea = undefined;
  unicode = { activeVersion: "" };
  buffer = {
    active: {
      type: "normal",
      viewportY: 0,
      baseY: 0,
      cursorX: 0,
      cursorY: 0,
      length: 24,
      getLine: () => undefined,
    },
  };
  constructor() {
    return new Proxy(this, {
      get: (t, p) => (p in t ? t[p as keyof FakeTerminal] : stub()),
    });
  }
  loadAddon(addon: { activate?: (t: unknown) => void }) {
    addon.activate?.(this);
  }
  resize(cols: number, rows: number) {
    this.cols = cols;
    this.rows = rows;
  }
  write(_data: unknown, done?: () => void) {
    if (done) setTimeout(done, 0);
  }
}

class FakeAddon {
  activate() {}
  dispose() {}
  fit() {}
  serialize() {
    return "";
  }
  onDidChangeResults() {
    return { dispose: () => undefined };
  }
}

vi.mock("@xterm/xterm", () => ({ Terminal: FakeTerminal }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: FakeAddon }));
vi.mock("@xterm/addon-search", () => ({ SearchAddon: FakeAddon }));
vi.mock("@xterm/addon-serialize", () => ({ SerializeAddon: FakeAddon }));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: FakeAddon }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: FakeAddon }));
vi.mock("@xterm/addon-webgl", () => ({ WebglAddon: FakeAddon }));

const { TerminalView } = await import("./TerminalView");

const OPTIONS = {
  theme: {} as never,
  fontFamily: "JetBrains Mono",
  fontSize: 13,
  cursorStyle: "block" as const,
  cursorBlink: false,
  scrollback: 10_000,
  gpu: false,
};

afterEach(() => setInvokeBackend(null));

describe("the grid a waiting view starts its pane at (F-CD-03.2)", () => {
  it("F-CD-03.2 is the last fit's for a host of its size, scaled for another size, and none before any fit", () => {
    expect(guessSize("13px Other", 800, 600)).toBeNull();
    rememberFit("13px Mono", { width: 800, height: 500, cols: 99, rows: 30 });
    expect(guessSize("13px Mono", 800, 500)).toEqual({ cols: 99, rows: 30 });
    expect(guessSize("13px Mono", 400, 250)).toEqual({ cols: 49, rows: 15 });
    expect(guessSize("13px Mono", 0, 0)).toEqual({ cols: 2, rows: 2 });
    // Another font's cells are another size.
    expect(guessSize("14px Mono", 800, 500)).toBeNull();
  });
});

describe("views waiting to open start their panes first (F-CD-03.2)", () => {
  it("F-CD-03.2 a tab's waiting views start their panes before they open, then attach", async () => {
    const calls: Array<{ cmd: string; paneId: string; args: Record<string, unknown> }> = [];
    setInvokeBackend(async (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      calls.push({ cmd, paneId: String(a.paneId ?? ""), args: a });
      if (cmd === "pty_start") return true;
      if (cmd === "pty_attach") return { replay: "", end: 0, live: true, state: null };
      if (cmd === "pty_spawn") return { pid: 1, startedAt: 0 };
      return null;
    });
    // A tab of three panes the same size; jsdom lays nothing out.
    const width = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(800);
    const height = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(500);
    try {
      const panes = ["pn_a1", "pn_a2", "pn_a3"];
      render(
        <>
          {panes.map((id) => (
            <TerminalView key={id} paneId={id} label="agent" visible options={OPTIONS} />
          ))}
        </>,
      );
      await waitFor(() => expect(calls.filter((c) => c.cmd === "pty_attach")).toHaveLength(3));
    } finally {
      width.mockRestore();
      height.mockRestore();
    }
    const at = (cmd: string, id: string) => calls.findIndex((c) => c.cmd === cmd && c.paneId === id);
    // The first opens at once and connects as it always has.
    expect(at("pty_start", "pn_a1")).toBe(-1);
    for (const id of ["pn_a2", "pn_a3"]) {
      expect(at("pty_start", id)).toBeGreaterThanOrEqual(0);
      expect(at("pty_start", id)).toBeLessThan(at("pty_attach", id));
      // At the grid the first view fit.
      expect(calls[at("pty_start", id)]?.args).toMatchObject({ cols: 80, rows: 24 });
    }
    expect(calls.filter((c) => c.cmd === "pty_spawn")).toHaveLength(0);
  });
});
