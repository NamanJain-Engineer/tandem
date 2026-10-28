/** The app's side of the core's `untrusted` module (F-TL-06 rule 8), for the
 * prompts the UI builds itself. `crates/tandem-core/tests/fixtures/defang.json`
 * pins both to the same text (D-3023). */

/** What a prompt adds after text `defangMentions` changed. It holds no `@`
 * itself, so it names nothing. */
export const DEFANGED =
  "Vibezeo wrote ＠ for each at sign in the text above, so none of it attaches a file; read ＠ as an at sign.";

/** `text` with every `@` that could start a mention made `＠`: Claude Code
 * attaches the file an `@path` in a message names, with no tool call
 * (D-2839). A run of `@` stays, whole, only when a space, a tab or a line
 * break follows it; any other run becomes `＠`, all of it, the end of the
 * text included, since a run joins whatever follows it (D-3025). */
export function defangMentions(text: string): string {
  return text.replace(/@+(?![@ \t\n\r])/g, (run) => "\uFF20".repeat(run.length));
}
