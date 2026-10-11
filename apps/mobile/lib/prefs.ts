// The phone's own settings (F-RM-05): in-app notifications (on by default,
// they never leave the phone) and push (F-RM-06, off until the person turns
// it on). Kept in secure storage beside the pairing.
import type { SecureKv } from "./secureStore";

export type Prefs = { notifications: boolean; push: boolean };

export const DEFAULT_PREFS: Prefs = { notifications: true, push: false };
const KEY = "vibezeo.prefs";

export async function loadPrefs(kv: SecureKv): Promise<Prefs> {
  try {
    const raw = await kv.getItemAsync(KEY);
    const v = raw ? (JSON.parse(raw) as Partial<Prefs>) : {};
    return {
      notifications: typeof v.notifications === "boolean" ? v.notifications : DEFAULT_PREFS.notifications,
      push: typeof v.push === "boolean" ? v.push : DEFAULT_PREFS.push,
    };
  } catch {
    return { ...DEFAULT_PREFS };
  }
}

export async function savePrefs(kv: SecureKv, prefs: Prefs): Promise<void> {
  await kv.setItemAsync(KEY, JSON.stringify(prefs));
}
