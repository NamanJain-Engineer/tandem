// Terminal keys, copy and paste (F-CD-05, F-CD-07, ARCHITECTURE §5.9). The
// app-wide dispatcher has already taken registry shortcuts; what reaches here
// is decided per key: copy, paste, the agent newline, or the PTY.

export type KeyLike = Pick<KeyboardEvent, "type" | "key" | "code" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey" | "isComposing">;

export type KeyContext = {
  mac: boolean;
  hasSelection: boolean;
  /** An agent pane (Shift+Enter types the engine's newline). */
  agent: boolean;
};

export type KeyAction = "copy" | "paste" | "newline" | "pass";

export function keyAction(e: KeyLike, ctx: KeyContext): KeyAction {
  if (e.type !== "keydown" || e.isComposing) return "pass";
  const only = (mods: { ctrl?: boolean; meta?: boolean; shift?: boolean; alt?: boolean }) =>
    e.ctrlKey === !!mods.ctrl && e.metaKey === !!mods.meta && e.shiftKey === !!mods.shift && e.altKey === !!mods.alt;
  const c = e.code === "KeyC";
  const v = e.code === "KeyV";
  if (ctx.mac) {
    if (c && only({ meta: true })) return ctx.hasSelection ? "copy" : "pass";
    if (v && only({ meta: true })) return "paste";
  } else {
    // Ctrl+C copies a selection and otherwise is SIGINT (0x03); Ctrl+Shift+C
    // always copies. Ctrl+V stays the shell's (literal next).
    if (c && only({ ctrl: true })) return ctx.hasSelection ? "copy" : "pass";
    if (c && only({ ctrl: true, shift: true })) return "copy";
    if (v && only({ ctrl: true, shift: true })) return "paste";
  }
  if (e.key === "Insert" && only({ shift: true })) return "paste";
  if (e.key === "Enter" && only({ shift: true }) && ctx.agent) return "newline";
  return "pass";
}

/** Lines in pasted text (a trailing newline doesn't start another). */
export function lineCount(text: string): number {
  const body = text.replace(/\r?\n$/, "");
  return body === "" ? 0 : body.split(/\r\n|\r|\n/).length;
}

/** Whether a paste asks "Paste N lines?" first (shells only, F-CD-05). */
export function needsConfirm(text: string, opts: { agent: boolean; confirmSetting: boolean }): boolean {
  return !opts.agent && opts.confirmSetting && lineCount(text) > 1;
}

const PASTE_START = "\x1b[200~";
const PASTE_END = "\x1b[201~";

/** The bytes a paste sends (F-CD-05 AC2). With bracketed paste on, the text
 * goes between the markers with its line breaks as LF, and any marker inside
 * it is removed so pasted text can't end the bracket early and run the rest.
 * Without it, line breaks become CR (Enter), as terminals type them. */
export function pasteBytes(text: string, bracketed: boolean): string {
  if (!bracketed) return text.replace(/\r?\n/g, "\r");
  const body = text.replace(/\r\n?/g, "\n").split(PASTE_START).join("").split(PASTE_END).join("");
  return `${PASTE_START}${body}${PASTE_END}`;
}

/** The last `n` lines of a buffer as plain text (`null` = all), trailing blank lines dropped. */
export function lastLines(lines: string[], n: number | null): string {
  let end = lines.length;
  while (end > 0 && (lines[end - 1] ?? "").trim() === "") end--;
  const start = n === null ? 0 : Math.max(0, end - n);
  return lines.slice(start, end).join("\n");
}
