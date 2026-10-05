/** React `useId` values depend on render order; number them by first
 * appearance so snapshots are stable when tests are filtered or reordered. */
export function stableIds(html: string): string {
  const seen = new Map<string, string>();
  return html.replace(/_r_[0-9a-z]+_/g, (id) => {
    let next = seen.get(id);
    if (!next) {
      next = `_id${seen.size}_`;
      seen.set(id, next);
    }
    return next;
  });
}
