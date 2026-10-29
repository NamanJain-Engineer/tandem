/** The app's side of the core's `untrusted` module (F-TL-06 rule 8), for the
 * prompts the UI builds itself. `crates/tandem-core/tests/fixtures/defang.json`
 * pins both to the same text (D-3023). */

/** What a prompt adds after `defang` wrote ＠. It holds no `@` itself, so it
 * names nothing. */
export const DEFANGED =
  "Vibezeo wrote ＠ for each at sign in the text above, so none of it attaches a file; read ＠ as an at sign.";

/** What text from elsewhere loses before anything else (D-3030): everything
 * Claude Code's prompt drops before it reads a message, and more. The same
 * code points as the core's `untrusted::removable`, which the shared vectors
 * pin, rather than this engine's Unicode tables. */
const REMOVABLE: ReadonlyArray<readonly [number, number]> = [
  [0x00, 0x08],
  [0x0b, 0x0c],
  [0x0e, 0x1f],
  [0x7f, 0x9f],
  [0xad, 0xad],
  [0x34f, 0x34f],
  [0x600, 0x605],
  [0x61c, 0x61c],
  [0x6dd, 0x6dd],
  [0x70f, 0x70f],
  [0x890, 0x891],
  [0x8e2, 0x8e2],
  [0x115f, 0x1160],
  [0x17b4, 0x17b5],
  [0x180b, 0x180f],
  [0x200b, 0x200f],
  [0x2028, 0x202e],
  [0x2060, 0x206f],
  [0x2800, 0x2800],
  [0x3164, 0x3164],
  [0xfe00, 0xfe0f],
  [0xfeff, 0xfeff],
  [0xffa0, 0xffa0],
  [0xfff0, 0xfffb],
  [0x1107f, 0x1107f],
  [0x110bd, 0x110bd],
  [0x110cd, 0x110cd],
  [0x13430, 0x1343f],
  [0x16fe4, 0x16fe4],
  [0x1bca0, 0x1bca3],
  [0x1d173, 0x1d17a],
  [0xe0000, 0xe0fff],
];

/** Whether text from elsewhere loses the code point `cp` first. */
export function removable(cp: number): boolean {
  return REMOVABLE.some(([lo, hi]) => cp >= lo && cp <= hi);
}

/** `text` without what's removable, with the one character NFC makes an
 * ASCII letter (U+212A KELVIN SIGN) as that letter: Claude Code looks for its
 * words in a paste's NFC form too. */
function readable(text: string): string {
  return Array.from(text)
    .filter((ch) => !removable(ch.codePointAt(0) ?? 0))
    .map((ch) => (ch === "\u212A" ? "K" : ch))
    .join("");
}

/** The `[` that opens one of Claude Code's paste markers (`[Pasted text #1]`,
 * `[Image #2]`). Without the `u` flag, `i` folds ASCII letters only, as the
 * core does. */
const PASTE_MARKER = /\[(?=(?:Pasted text|Image|\.\.\.Truncated text|\u2726 Team setup guide) #\d)/gi;

/** `ultra` before a word Claude Code acts on in a message (`ultrathink`,
 * `ultraplan`), in any ASCII case. */
const ULTRA_WORD = /(ultra)(?=code|plan|review|think)/gi;

/** A `/` that opens a word Claude Code could read as a skill's name, which
 * its Skill tool takes as typed by you (`(?<!\S)/name(?=$|\s)`, D-3033): the
 * word runs to the next whitespace with no second `/` (a skill's name is a
 * folder's, so a path keeps its slashes) and starts with a letter, a digit,
 * `_` or anything outside ASCII. Written without lookbehind, which older
 * WebKitGTK lacks. */
const SKILL_WORD = /(^|\s)\/(?=(?:[A-Za-z0-9_]|\P{ASCII})[^\s/]*(?!\S))/gu;

/** `defang`, and whether it wrote ＠, which `DEFANGED` explains. */
export function defangMarked(text: string): { text: string; wroteAt: boolean } {
  const shown = readable(text);
  // A run of `@` stays, whole, only when a space, a tab or a line break
  // follows it; any other run becomes `＠`, all of it, the end of the text
  // included, since a run joins whatever follows it (D-3025).
  const mentions = shown.replace(/@+(?![@ \t\n\r])/g, (run) => "\uFF20".repeat(run.length));
  return {
    text: mentions.replace(SKILL_WORD, "$1\uFF0F").replace(PASTE_MARKER, "\uFF3B").replace(ULTRA_WORD, "$1-"),
    wroteAt: mentions !== shown,
  };
}

/** Text from elsewhere, made safe to put in a message to an engine: what's
 * removable goes first, no `@` names a file (Claude Code attaches the file
 * an `@path` names, with no tool call, D-2839), no word names a skill
 * (D-3033), no paste marker stands in for a paste, and no word Claude Code
 * acts on is left whole (D-3029). */
export function defang(text: string): string {
  return defangMarked(text).text;
}
