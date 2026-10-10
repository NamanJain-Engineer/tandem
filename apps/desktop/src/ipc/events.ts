import { inTauri, usingFakeBackend } from "./invoke";

type Handler<T> = (payload: T) => void;
const fake = new Map<string, Set<Handler<unknown>>>();

/** Subscribes to a Rust → webview event (ARCHITECTURE §3.3). Returns an
 * unsubscribe function. Tests and the browser preview use a local bus. */
export async function listen<T>(event: string, handler: Handler<T>): Promise<() => void> {
  if (inTauri() && !usingFakeBackend()) {
    const api = await import("@tauri-apps/api/event");
    return api.listen<T>(event, (e) => handler(e.payload));
  }
  const set = fake.get(event) ?? new Set<Handler<unknown>>();
  set.add(handler as Handler<unknown>);
  fake.set(event, set);
  return () => set.delete(handler as Handler<unknown>);
}

/** Delivers an event to local listeners (tests, preview). */
export function emitLocal<T>(event: string, payload: T): void {
  fake.get(event)?.forEach((h) => h(payload));
}

/** Sends an event to another window (the widget → the main window). */
export async function emitTo<T>(target: string, event: string, payload: T): Promise<void> {
  if (inTauri() && !usingFakeBackend()) {
    const api = await import("@tauri-apps/api/event");
    await api.emitTo(target, event, payload);
    return;
  }
  emitLocal(event, payload);
}
