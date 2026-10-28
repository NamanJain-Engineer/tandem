/** The app's side of the core's `untrusted` module (F-TL-06 rule 8), for the
 * prompts the UI builds itself. `crates/tandem-core/tests/fixtures/defang.json`
 * pins both to the same text (D-3023). */

/** What a prompt adds after `defang` wrote ＠. It holds no `@` itself, so it
 * names nothing. */
export const DEFANGED =
  "Vibezeo wrote ＠ for each at sign in the text above, so none of it attaches a file; read ＠ as an at sign.";

/** What nothing shows: the default-ignorable code points, as ranges the
 * core lists too rather than this engine's Unicode tables (D-3029). */
const INVISIBLE: ReadonlyArray<readonly [number, number]> = [
  [0x00ad, 0x00ad],
  [0x034f, 0x034f],
  [0x061c, 0x061c],
  [0x115f, 0x1160],
  [0x17b4, 0x17b5],
  [0x180b, 0x180f],
  [0x200b, 0x200f],
  [0x202a, 0x202e],
  [0x2060, 0x206f],
  [0x3164, 0x3164],
  [0xfe00, 0xfe0f],
  [0xfeff, 0xfeff],
  [0xffa0, 0xffa0],
  [0xfff0, 0xfff8],
  [0x1bca0, 0x1bca3],
  [0x1d173, 0x1d17a],
  [0xe0000, 0xe0fff],
];

function visible(ch: string): boolean {
  const cp = ch.codePointAt(0) ?? 0;
  return !INVISIBLE.some(([lo, hi]) => cp >= lo && cp <= hi);
}

/** The `[` that opens one of Claude Code's paste markers (`[Pasted text #1]`,
 * `[Image #2]`). Without the `u` flag, `i` folds ASCII letters only, as the
 * core does. */
const PASTE_MARKER = /\[(?=(?:Pasted text|Image|\.\.\.Truncated text|\u2726 Team setup guide) #\d)/gi;

/** `ultra` before a word Claude Code acts on in a message (`ultrathink`,
 * `ultraplan`), in any ASCII case. */
const ULTRA_WORD = /(ultra)(?=code|plan|review|think)/gi;

/** `defang`, and whether it wrote ＠, which `DEFANGED` explains. */
export function defangMarked(text: string): { text: string; wroteAt: boolean } {
  const shown = Array.from(text).filter(visible).join("");
  // A run of `@` stays, whole, only when a space, a tab or a line break
  // follows it; any other run becomes `＠`, all of it, the end of the text
  // included, since a run joins whatever follows it (D-3025).
  const mentions = shown.replace(/@+(?![@ \t\n\r])/g, (run) => "\uFF20".repeat(run.length));
  return {
    text: mentions.replace(PASTE_MARKER, "\uFF3B").replace(ULTRA_WORD, "$1-"),
    wroteAt: mentions !== shown,
  };
}

/** Text from elsewhere, made safe to put in a message to an engine: what
 * nothing shows is dropped, no `@` names a file (Claude Code attaches the file
 * an `@path` names, with no tool call, D-2839), no paste marker stands in for
 * a paste, and no word Claude Code acts on is left whole (D-3029). */
export function defang(text: string): string {
  return defangMarked(text).text;
}
