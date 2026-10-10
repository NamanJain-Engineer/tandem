// Parking a hidden terminal (PRODUCT_SPEC §5.1 memory, D-139). After a pane
// has been hidden for PARK_AFTER_MS its xterm is serialized into the core's
// ring and disposed; showing it again replays the ring into a new one. The
// base is one chunk per logical line, each starting from the default pen, so
// the ring can drop whole lines from the front (its usual trimming) without
// changing the colours of the lines it keeps. The last chunk restores the
// screen: cursor, pen, scroll region, alternate screen, modes, mouse encoding,
// cursor visibility and style, and the DEC line-drawing charsets. Command
// marks (F-CD-10) ride along as the OSC 133 sequences that made them.

import type { SerializeAddon } from "@xterm/addon-serialize";

export const PARK_AFTER_MS = 30_000;

/** A command mark on a buffer line: ✓/✗ (`done`), a command still running,
 * or the current prompt no command ran from yet. */
export type ParkMark = { line: number } & (
  | { kind: "done"; ok: boolean }
  | { kind: "running" }
  | { kind: "prompt" }
);

/** The cell attribute API xterm's buffer cells and pen share. */
export type Attr = {
  isBold(): number;
  isItalic(): number;
  isDim(): number;
  isUnderline(): number;
  isBlink(): number;
  isInverse(): number;
  isInvisible(): number;
  isStrikethrough(): number;
  isOverline(): number;
  isFgRGB(): boolean;
  isBgRGB(): boolean;
  isFgPalette(): boolean;
  isBgPalette(): boolean;
  getFgColorMode(): number;
  getBgColorMode(): number;
  getFgColor(): number;
  getBgColor(): number;
};

type Line = { isWrapped: boolean };
type Buffer = {
  type: "normal" | "alternate";
  length: number;
  cursorX: number;
  cursorY: number;
  baseY: number;
  viewportY: number;
  getLine(y: number): Line | undefined;
};

/** What parking needs from a terminal: the public API of `@xterm/xterm` and
 * `@xterm/headless`, plus internals read defensively (absent ones are skipped). */
export type ParkTerminal = {
  cols: number;
  rows: number;
  buffer: { normal: Buffer; alternate: Buffer; active: Buffer };
  modes: {
    applicationCursorKeysMode: boolean;
    applicationKeypadMode: boolean;
    bracketedPasteMode: boolean;
    insertMode: boolean;
    originMode: boolean;
    reverseWraparoundMode: boolean;
    sendFocusMode: boolean;
    wraparoundMode: boolean;
    mouseTrackingMode: "none" | "x10" | "vt200" | "drag" | "any";
  };
  options: { cursorStyle?: string; cursorBlink?: boolean };
};

type Internals = {
  _core?: {
    _inputHandler?: { _curAttrData?: Attr };
    coreService?: { isCursorHidden?: boolean };
    coreMouseService?: { activeEncoding?: string };
    _charsetService?: { glevel?: number; _charsets?: Array<Record<string, string> | undefined> };
    buffers?: {
      active?: { scrollTop?: number; scrollBottom?: number };
      normal?: { savedCurAttrData?: Attr };
    };
  };
};

// xterm's colour modes (Attributes.CM_* in its buffer constants).
const CM_P16 = 0x1000000;
const CM_P256 = 0x2000000;
const CM_RGB = 0x3000000;

/** SGR for an attribute set, from the default pen. */
export function sgrOf(a: Attr): string {
  const p: Array<string | number> = [0];
  if (a.isBold()) p.push(1);
  if (a.isDim()) p.push(2);
  if (a.isItalic()) p.push(3);
  if (a.isUnderline()) p.push(4);
  if (a.isBlink()) p.push(5);
  if (a.isInverse()) p.push(7);
  if (a.isInvisible()) p.push(8);
  if (a.isStrikethrough()) p.push(9);
  if (a.isOverline()) p.push(53);
  // xterm keeps 16-colour (30–37, 90–97) and 256-colour (38;5) apart.
  const colour = (fg: boolean, mode: number, value: number) => {
    const base = fg ? 30 : 40;
    if (mode === CM_RGB) p.push(`${base + 8};2;${(value >> 16) & 255};${(value >> 8) & 255};${value & 255}`);
    else if (mode === CM_P256) p.push(`${base + 8};5;${value}`);
    else if (mode === CM_P16) p.push(value < 8 ? base + value : base + 60 + value - 8);
  };
  colour(true, a.getFgColorMode(), a.getFgColor());
  colour(false, a.getBgColorMode(), a.getBgColor());
  return `\x1b[${p.join(";")}m`;
}

function modesOf(term: ParkTerminal): string {
  const m = term.modes;
  let s = "";
  if (m.applicationCursorKeysMode) s += "\x1b[?1h";
  if (m.applicationKeypadMode) s += "\x1b[?66h";
  if (m.bracketedPasteMode) s += "\x1b[?2004h";
  if (m.insertMode) s += "\x1b[4h";
  if (m.reverseWraparoundMode) s += "\x1b[?45h";
  if (m.sendFocusMode) s += "\x1b[?1004h";
  if (!m.wraparoundMode) s += "\x1b[?7l";
  const mouse = { none: "", x10: "\x1b[?9h", vt200: "\x1b[?1000h", drag: "\x1b[?1002h", any: "\x1b[?1003h" };
  s += mouse[m.mouseTrackingMode] ?? "";
  const core = (term as unknown as Internals)._core;
  const encoding = core?.coreMouseService?.activeEncoding;
  if (encoding === "SGR") s += "\x1b[?1006h";
  else if (encoding === "SGR_PIXELS") s += "\x1b[?1016h";
  if (core?.coreService?.isCursorHidden) s += "\x1b[?25l";
  return s;
}

/** DEC special graphics in G0/G1 (line drawing), and the shift into G1. */
function charsetsOf(term: ParkTerminal): string {
  const cs = (term as unknown as Internals)._core?._charsetService;
  if (!cs) return "";
  const graphics = (c: Record<string, string> | undefined) => c?.q === "─";
  let s = "";
  if (graphics(cs._charsets?.[0])) s += "\x1b(0";
  if (graphics(cs._charsets?.[1])) s += "\x1b)0";
  if (cs.glevel === 1) s += "\x0e";
  return s;
}

/** DECSCUSR for a cursor style a program set (differs from `user`). */
function cursorStyleOf(term: ParkTerminal, user: { cursorStyle?: string; cursorBlink?: boolean }): string {
  const style = term.options.cursorStyle ?? "block";
  const blink = term.options.cursorBlink ?? false;
  if (style === (user.cursorStyle ?? "block") && blink === (user.cursorBlink ?? false)) return "";
  const n = { block: 1, underline: 3, bar: 5 }[style] ?? 1;
  return `\x1b[${blink ? n : n + 1} q`;
}

function marksFor(marks: ParkMark[]): string {
  let s = "";
  for (const m of marks) {
    s += "\x1b]133;A\x07";
    if (m.kind === "done") s += `\x1b]133;C\x07\x1b]133;D;${m.ok ? 0 : 1}\x07`;
    else if (m.kind === "running") s += "\x1b]133;C\x07";
  }
  return s;
}

/** The ring's new base for `term`: one chunk per logical line of the normal
 * buffer, then one that restores the screen. Replaying them, in order, into
 * a terminal of the same size gives the same buffers. */
export function parkChunks(
  term: ParkTerminal,
  serializer: SerializeAddon,
  marks: ParkMark[],
  user: { cursorStyle?: string; cursorBlink?: boolean } = {},
): string[] {
  const normal = term.buffer.normal;
  const core = (term as unknown as Internals)._core;
  // Marks by the first row of their logical line.
  const byLine = new Map<number, ParkMark[]>();
  for (const m of marks) {
    let y = m.line;
    while (y > 0 && normal.getLine(y)?.isWrapped) y--;
    byLine.set(y, [...(byLine.get(y) ?? []), m]);
  }
  const chunks: string[] = [];
  for (let start = 0; start < normal.length; ) {
    let end = start;
    while (end + 1 < normal.length && normal.getLine(end + 1)?.isWrapped) end++;
    const text = serializer.serialize({ range: { start, end }, excludeAltBuffer: true, excludeModes: true });
    const last = end + 1 >= normal.length;
    chunks.push(`${marksFor(byLine.get(start) ?? [])}${text}\x1b[0m${last ? "" : "\r\n"}`);
    start = end + 1;
  }
  const alt = term.buffer.active.type === "alternate";
  const pen = core?._inputHandler?._curAttrData;
  // Origin mode and a scroll region both home the cursor, so they come
  // before it's placed. The region is the active screen's.
  const region = core?.buffers?.active;
  const scroll =
    region?.scrollTop !== undefined &&
    region.scrollBottom !== undefined &&
    (region.scrollTop !== 0 || region.scrollBottom !== term.rows - 1)
      ? `\x1b[${region.scrollTop + 1};${region.scrollBottom + 1}r`
      : "";
  const origin = term.modes.originMode ? "\x1b[?6h" : "";
  let screen = "";
  if (!alt) {
    screen += `${scroll}${origin}\x1b[${normal.cursorY + 1};${normal.cursorX + 1}H`;
    if (pen) screen += sgrOf(pen);
  } else {
    // The normal screen's cursor and pen, which 1049 saves on the way in.
    screen += `\x1b[${normal.cursorY + 1};${normal.cursorX + 1}H`;
    const saved = core?.buffers?.normal?.savedCurAttrData;
    if (saved) screen += sgrOf(saved);
    // The addon's own alternate-screen serialization (cursor and pen included),
    // taken as the part past the normal screen's; it starts from the default pen.
    const both = serializer.serialize({ scrollback: 0, excludeModes: true });
    const normalOnly = serializer.serialize({ scrollback: 0, excludeModes: true, excludeAltBuffer: true });
    const enter = "\x1b[?1049h\x1b[H";
    const part = both.startsWith(normalOnly) ? both.slice(normalOnly.length) : enter;
    const content = part.startsWith(enter) ? part.slice(enter.length) : part;
    // A region or origin mode homes the cursor; the content then places it.
    screen += `${enter}\x1b[0m${scroll}${origin}\x1b[H${content}`;
  }
  screen += modesOf(term) + charsetsOf(term) + cursorStyleOf(term, user);
  chunks.push(screen);
  return chunks;
}
