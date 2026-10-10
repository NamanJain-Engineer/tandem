/** `@name` at the start of a teammate composer's message is mail (F-AG-12):
 * the longest teammate name it starts with (case-insensitive, then a space
 * or the end), other than the chat's own teammate. */
export function mailMention<T extends { id: string; name: string }>(
  text: string,
  teammates: T[],
  self: string,
): { to: T; body: string } | null {
  const trimmed = text.trimStart();
  if (!trimmed.startsWith("@")) return null;
  const rest = trimmed.slice(1);
  const lower = rest.toLocaleLowerCase();
  let best: T | null = null;
  for (const tm of teammates) {
    if (tm.id === self) continue;
    const name = tm.name.toLocaleLowerCase();
    if (!lower.startsWith(name)) continue;
    const next = rest.charAt(tm.name.length);
    if (next !== "" && !/\s/.test(next)) continue;
    if (!best || tm.name.length > best.name.length) best = tm;
  }
  if (!best) return null;
  return { to: best, body: rest.slice(best.name.length).trim() };
}
