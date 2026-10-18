import { act, render, waitFor } from "@testing-library/react";
import { setFlagsFromString } from "node:v8";
import { runInNewContext } from "node:vm";
import { afterEach, describe, expect, it, vi } from "vitest";
import { setInvokeBackend } from "../../ipc/invoke";

// A parked view hands its xterm to the core (§5.1, D-139). Disposing the
// xterm doesn't free its buffers: only collecting it does, so nothing the
// view leaves behind may still refer to it. xterm needs a real browser; a
// stand-in that answers every call shows what the view itself holds on to.

setFlagsFromString("--expose-gc");
const gc = runInNewContext("gc") as () => void;

const terminals: Array<WeakRef<object>> = [];

/** Anything called, read or disposed: a callable stand-in. */
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
    active: { type: "normal", viewportY: 0, baseY: 0, cursorX: 0, cursorY: 0, length: 24, getLine: () => undefined },
  };
  /** What a full scrollback would weigh. */
  lines = new Uint8Array(1 << 20);
  constructor() {
    const self = new Proxy(this, {
      get: (t, p) => (p in t ? t[p as keyof FakeTerminal] : stub()),
    });
    terminals.push(new WeakRef(self));
    return self;
  }
  loadAddon(addon: { activate?: (t: unknown) => void }) {
    addon.activate?.(this);
  }
  write(_data: unknown, done?: () => void) {
    if (done) setTimeout(done, 0);
  }
}

/** Addons keep the terminal they were loaded into, as xterm's do. */
class FakeAddon {
  terminal: unknown;
  activate(t: unknown) {
    this.terminal = t;
  }
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
vi.mock("./park", async (actual) => ({
  ...(await actual<typeof import("./park")>()),
  parkChunks: () => [],
}));

const { TerminalView, parkedPanes, setParkAfter } = await import("./TerminalView");

const OPTIONS = {
  theme: {} as never,
  fontFamily: "JetBrains Mono",
  fontSize: 13,
  cursorStyle: "block" as const,
  cursorBlink: false,
  scrollback: 10_000,
  gpu: false,
};

afterEach(() => {
  setInvokeBackend(null);
  setParkAfter(null);
});

describe("parked views let their xterm go (§5.1, D-139)", () => {
  it("§5.1 F-CD-04 a parked view's xterm can be collected while the core keeps its channel", async () => {
    // The core keeps the pane's channel while the view is parked.
    const channels: unknown[] = [];
    setInvokeBackend(async (cmd, args) => {
      if (cmd === "pty_attach") {
        channels.push(args?.channel);
        return { replay: "", end: 0, live: true, state: null };
      }
      return cmd === "pty_park" ? true : null;
    });
    setParkAfter(0);
    const view = (visible: boolean) => (
      <TerminalView paneId="pn_park" label="shell" visible={visible} options={OPTIONS} />
    );
    const { rerender } = render(view(true));
    await waitFor(() => expect(channels).toHaveLength(1));
    await waitFor(() => expect(terminals).toHaveLength(1));
    // Shown long enough to connect, then hidden: it parks.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    rerender(view(false));
    await waitFor(() => expect(parkedPanes.has("pn_park")).toBe(true));
    for (let i = 0; i < 5; i++) {
      await new Promise((r) => setTimeout(r, 0));
      gc();
    }
    expect(channels).toHaveLength(1);
    expect(terminals.filter((t) => t.deref() !== undefined)).toHaveLength(0);
  });
});
