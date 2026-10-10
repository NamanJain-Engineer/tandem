import { SerializeAddon } from "@xterm/addon-serialize";
import { Terminal } from "@xterm/headless";
import { describe, expect, it } from "vitest";
import { parkChunks, type ParkMark, type ParkTerminal } from "./park";

type Headless = Terminal & ParkTerminal;

function term(cols = 40, rows = 10, scrollback = 1000): { t: Headless; s: SerializeAddon } {
  const t = new Terminal({ cols, rows, scrollback, allowProposedApi: true }) as Headless;
  const s = new SerializeAddon();
  t.loadAddon(s as never);
  return { t, s };
}

const write = (t: Terminal, data: string) => new Promise<void>((r) => t.write(data, r));

/** Everything a viewer or a program could tell apart. */
async function state(t: Headless, s: SerializeAddon) {
  const lines = (b: ParkTerminal["buffer"]["normal"]) => {
    const out: string[] = [];
    for (let y = 0; y < b.length; y++) {
      const l = (t.buffer.normal === b ? t.buffer.normal : t.buffer.alternate).getLine(y);
      out.push(`${l?.isWrapped ? "~" : ""}${l?.translateToString() ?? ""}`);
    }
    return out;
  };
  const core = (t as unknown as { _core: Record<string, never> })._core as unknown as {
    _inputHandler: { _curAttrData: { fg: number; bg: number } };
    coreService: { isCursorHidden: boolean };
    coreMouseService: { activeEncoding: string; activeProtocol: string };
    _charsetService: { glevel: number; charset?: Record<string, string> };
    buffers: { active: { scrollTop: number; scrollBottom: number } };
  };
  return {
    serialized: s.serialize(),
    normal: lines(t.buffer.normal),
    alternate: t.buffer.active.type === "alternate" ? lines(t.buffer.alternate) : null,
    active: t.buffer.active.type,
    cursor: [t.buffer.active.cursorX, t.buffer.active.cursorY, t.buffer.active.baseY],
    normalCursor: [t.buffer.normal.cursorX, t.buffer.normal.cursorY, t.buffer.normal.baseY],
    pen: [core._inputHandler._curAttrData.fg, core._inputHandler._curAttrData.bg],
    modes: { ...t.modes },
    hidden: core.coreService.isCursorHidden,
    mouse: [core.coreMouseService.activeEncoding, core.coreMouseService.activeProtocol],
    charset: [core._charsetService.glevel, core._charsetService.charset?.q ?? null],
    region: [core.buffers.active.scrollTop, core.buffers.active.scrollBottom],
    cursorStyle: [t.options.cursorStyle, t.options.cursorBlink],
  };
}

async function replay(chunks: string[], cols = 40, rows = 10, scrollback = 1000) {
  const r = term(cols, rows, scrollback);
  for (const c of chunks) await write(r.t, c);
  return r;
}

async function busyShell(t: Terminal) {
  for (let i = 0; i < 60; i++) {
    await write(t, `\x1b[3${i % 8}mline ${i}\x1b[1m bold\x1b[22;4m under\x1b[0m plain\r\n`);
  }
  // A line that wraps twice, true colour, a wide character and a coloured tail.
  await write(t, `\x1b[38;2;10;200;30m${"x".repeat(70)}界${"y".repeat(20)}\x1b[44m   \x1b[0m\r\n`);
  await write(t, "\x1b[7minverse\x1b[0m \x1b[9mstruck\x1b[0m \x1b[2mdim\x1b[0m \x1b[3mitalic\x1b[0m\r\n");
  // The prompt, its pen still green.
  await write(t, "\x1b[32m$ ");
}

describe("NFR-5.1 parking a hidden terminal (D-139)", () => {
  it("NFR-5.1 replaying the parked chunks gives the same scrollback, screen, cursor and pen", async () => {
    const { t, s } = term();
    await busyShell(t);
    const chunks = parkChunks(t, s, []);
    const r = await replay(chunks);
    expect(await state(r.t, r.s)).toEqual(await state(t, s));
  });

  it("NFR-5.1 the alternate screen survives with its content, cursor, pen and scroll region, and the normal screen under it", async () => {
    const { t, s } = term();
    await busyShell(t);
    // A full-screen program: alternate screen, a region, mouse in SGR, hidden
    // cursor, line drawing, bracketed paste, application cursor keys and a bar cursor.
    await write(t, "\x1b[?1049h\x1b[H\x1b[2J\x1b[?1002h\x1b[?1006h\x1b[?25l\x1b[?2004h\x1b[?1h\x1b[5 q");
    await write(t, "\x1b[1;1H\x1b[45mtitle bar\x1b[0m\x1b[2;9r\x1b[3;3H\x1b(0lqqk\x1b(B\x1b[5;5H\x1b[33mediting");
    await write(t, "\x1b(0");
    const chunks = parkChunks(t, s, [], { cursorStyle: "block", cursorBlink: false });
    const r = await replay(chunks);
    expect(await state(r.t, r.s)).toEqual(await state(t, s));
    // Leaving the program returns to the shell exactly as it was.
    await write(t, "\x1b[?1049l");
    await write(r.t, "\x1b[?1049l");
    expect(await state(r.t, r.s)).toEqual(await state(t, s));
  });

  it("NFR-5.1 output after the park continues where the program left off (the core's ring tail)", async () => {
    const { t, s } = term();
    await busyShell(t);
    const chunks = parkChunks(t, s, []);
    const r = await replay(chunks);
    const tail = "ls\r\n\x1b[34mdir\x1b[0m file\r\n\x1b[32m$ ";
    await write(t, tail);
    await write(r.t, tail);
    expect(await state(r.t, r.s)).toEqual(await state(t, s));
  });

  it("NFR-5.1 F-CD-10 command marks come back on their lines: ✓, ✗, a running command and the open prompt", async () => {
    const { t, s } = term();
    await busyShell(t);
    const marks: ParkMark[] = [
      { line: 3, kind: "done", ok: true },
      { line: 10, kind: "done", ok: false },
      { line: 61, kind: "running" },
      { line: 64, kind: "prompt" },
    ];
    const chunks = parkChunks(t, s, marks);
    const r = term();
    const seen: Array<{ line: number; data: string }> = [];
    r.t.parser.registerOscHandler(133, (data) => {
      seen.push({ line: r.t.buffer.active.baseY + r.t.buffer.active.cursorY, data });
      return true;
    });
    for (const c of chunks) await write(r.t, c);
    expect(seen).toEqual([
      { line: 3, data: "A" },
      { line: 3, data: "C" },
      { line: 3, data: "D;0" },
      { line: 10, data: "A" },
      { line: 10, data: "C" },
      { line: 10, data: "D;1" },
      { line: 60, data: "A" }, // line 61 continues the wrapped line 60
      { line: 60, data: "C" },
      { line: 64, data: "A" },
    ]);
  });

  it("NFR-5.1 the ring can drop whole lines from the front: the lines kept look exactly as they did", async () => {
    const { t, s } = term();
    await busyShell(t);
    const chunks = parkChunks(t, s, []);
    const r = await replay(chunks.slice(20));
    const rows = (x: ReturnType<typeof term>, from: number, to: number) =>
      Array.from({ length: to - from }, (_, i) => x.s.serialize({ range: { start: from + i, end: from + i } }));
    const orig = await state(t, s);
    const kept = await state(r.t, r.s);
    expect(kept.normal).toEqual(orig.normal.slice(20));
    expect(rows(r, 0, 30)).toEqual(rows({ t, s }, 20, 50));
  });

  it("NFR-5.1 every chunk but the screen's holds one line, so the ring counts lines as xterm does", () => {
    const { t, s } = term();
    return busyShell(t).then(() => {
      const chunks = parkChunks(t, s, []);
      const lines = chunks.slice(0, -1);
      expect(lines.every((c, i) => (c.match(/\n/g) ?? []).length === (i === lines.length - 1 ? 0 : 1))).toBe(true);
      // 60 + the wrapped line (one logical line) + 1 + the prompt line.
      expect(lines.length).toBe(63);
    });
  });
});
