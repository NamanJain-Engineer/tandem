// Each pane renders into a host element that the grid moves between layout
// slots. Swapping, moving or maximizing a pane moves its host, so the
// terminal (and its WebGL context and scrollback) is never torn down.

const hosts = new Map<string, HTMLDivElement>();
/** Focus inside a host that left the DOM, restored when it's attached again. */
let pendingFocus: { host: HTMLElement; el: HTMLElement } | null = null;

export function paneHost(paneId: string): HTMLDivElement {
  let el = hosts.get(paneId);
  if (!el) {
    el = document.createElement("div");
    el.className = "h-full w-full min-h-0 min-w-0";
    el.dataset.paneHost = paneId;
    hosts.set(paneId, el);
  }
  return el;
}

function detach(host: Element): void {
  const focused = document.activeElement;
  if (host instanceof HTMLElement && focused instanceof HTMLElement && host.contains(focused))
    pendingFocus = { host, el: focused };
  host.remove();
}

/** Puts a pane's host into a slot, keeping keyboard focus inside it. */
export function attachPaneHost(paneId: string, slot: HTMLElement): void {
  const el = paneHost(paneId);
  // A slot shows one pane: drop a host left behind by a pane that moved away.
  for (const child of Array.from(slot.children)) if (child !== el) detach(child);
  if (el.parentElement === slot) return;
  if (el.isConnected) detach(el);
  slot.appendChild(el);
  if (pendingFocus?.host === el) {
    const target = pendingFocus.el;
    pendingFocus = null;
    target.focus({ preventScroll: true });
  }
}

/** Forgets a closed pane's host. */
export function releasePaneHost(paneId: string): void {
  hosts.get(paneId)?.remove();
  hosts.delete(paneId);
}
