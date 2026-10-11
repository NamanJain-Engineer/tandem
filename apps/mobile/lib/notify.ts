// Local notifications (F-RM-05.3): what the session's notices become on this
// phone. Nothing here leaves the device: the text comes from the desktop over
// the encrypted channel and is shown by the OS's local notification API.
import type { Notice } from "@tandem/remote/session";
import type { Prefs } from "./prefs";

export type AppPhase = "active" | "background" | "inactive";

/** Shows a notification now (expo-notifications in the app; a fake in tests). */
export type NotificationSink = (n: { title: string; body: string; id: string }) => void;

/** In-app banners while the app is in front; OS notifications otherwise. */
export type Route = "banner" | "os" | "none";

export function routeNotice(prefs: Prefs, phase: AppPhase): Route {
  if (!prefs.notifications) return "none";
  return phase === "active" ? "banner" : "os";
}

/** Turns a notice into what's shown; long text is cut to fit. */
export function present(notice: Notice): { title: string; body: string; id: string } {
  const clip = (s: string, n: number) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);
  return { id: `${notice.kind}:${notice.id}`, title: clip(notice.title, 80), body: clip(notice.body, 160) };
}

/** Remembers what was shown so a reconnect doesn't repeat it. */
export class NoticeGate {
  private readonly seen = new Set<string>();

  constructor(private readonly max = 200) {}

  admit(notice: Notice): boolean {
    const key = `${notice.kind}:${notice.id}:${notice.title}`;
    if (this.seen.has(key)) return false;
    this.seen.add(key);
    if (this.seen.size > this.max) {
      const first = this.seen.values().next().value;
      if (first !== undefined) this.seen.delete(first);
    }
    return true;
  }
}
