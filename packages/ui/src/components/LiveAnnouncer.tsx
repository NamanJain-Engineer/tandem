import { useSyncExternalStore } from "react";

/** §5.4: status changes are announced politely, at most once per 2 s. */
export const ANNOUNCE_INTERVAL_MS = 2000;
/** Messages folded into one announcement before the rest is counted. */
const MAX_PER_ANNOUNCEMENT = 3;

/** How the overflow reads; the app sets it from its catalog. */
let more: (count: number) => string = (count) => `And ${count} more`;

export function configureAnnouncer(opts: { more: (count: number) => string }): void {
  more = opts.more;
}

let current = "";
let lastAt = Number.NEGATIVE_INFINITY;
let pending: string[] = [];
let timer: ReturnType<typeof setTimeout> | null = null;
const listeners = new Set<() => void>();

function publish(text: string): void {
  current = text;
  lastAt = Date.now();
  for (const l of listeners) l();
}

function flush(): void {
  timer = null;
  if (pending.length === 0) return;
  const shown = pending.slice(0, MAX_PER_ANNOUNCEMENT);
  const rest = pending.length - shown.length;
  pending = [];
  publish(rest > 0 ? `${shown.join(". ")}. ${more(rest)}` : shown.join(". "));
}

/** Queue a message for screen readers. Messages that arrive within the
 * interval are joined into the next announcement, duplicates once. */
export function announce(message: string): void {
  const text = message.trim();
  if (!text) return;
  if (!pending.includes(text)) pending.push(text);
  if (timer) return;
  const wait = lastAt + ANNOUNCE_INTERVAL_MS - Date.now();
  if (wait <= 0) flush();
  else timer = setTimeout(flush, wait);
}

/** Tests. */
export function resetAnnouncer(): void {
  if (timer) clearTimeout(timer);
  timer = null;
  pending = [];
  lastAt = Number.NEGATIVE_INFINITY;
  current = "";
  for (const l of listeners) l();
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

/** The app's one polite live region. Mount it once, near the root. */
export function LiveAnnouncer() {
  const text = useSyncExternalStore(subscribe, () => current);
  return (
    <div role="status" aria-live="polite" aria-atomic="true" className="sr-only" data-testid="live-announcer">
      {text}
    </div>
  );
}
