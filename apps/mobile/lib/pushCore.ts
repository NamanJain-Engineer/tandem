// Push on the phone (F-RM-06), without React Native imports so tests run it:
// what's kept (the pairing id and the alert key, in the Keychain / Keystore),
// and opening an alert the push service delivered.
import { b64url, fromB64url } from "@tandem/remote/protocol";
import { openPushPayload, type PushAlert } from "@tandem/remote/push";
import type { SecureKv } from "./secureStore";

export const PUSH_KEY = "vibezeo.push";

export type PushSecret = { pid: string; key: string; lastSeq: string };

export async function loadPushSecret(kv: SecureKv, options?: object): Promise<PushSecret | null> {
  try {
    const raw = await kv.getItemAsync(PUSH_KEY, options);
    return raw ? (JSON.parse(raw) as PushSecret) : null;
  } catch {
    return null;
  }
}

export async function savePushSecret(kv: SecureKv, secret: PushSecret, options?: object): Promise<void> {
  await kv.setItemAsync(PUSH_KEY, JSON.stringify(secret), options);
}

export async function clearPushSecret(kv: SecureKv, options?: object): Promise<void> {
  await kv.deleteItemAsync(PUSH_KEY, options);
}

/** A fresh alert key for one pairing. */
export function newPushSecret(pid: string, random: (n: number) => Uint8Array): PushSecret {
  return { pid, key: b64url(random(32)), lastSeq: "0" };
}

/** The sealed payload in an FCM data message (`{v:"1", p}`), however the OS hands it over. */
export function payloadOf(data: unknown): string | null {
  let d = data;
  if (typeof d === "string") {
    try {
      d = JSON.parse(d);
    } catch {
      return null;
    }
  }
  if (!d || typeof d !== "object") return null;
  const o = d as Record<string, unknown>;
  if (o.v === "1" && typeof o.p === "string") return o.p;
  for (const k of ["data", "dataString", "body"]) {
    const inner = o[k] !== undefined ? payloadOf(o[k]) : null;
    if (inner) return inner;
  }
  return null;
}

/**
 * Opens an alert. Null when it isn't for this pairing, doesn't authenticate,
 * or is older than one already shown (a replay).
 */
export function openAlert(secret: PushSecret, payload: string): { alert: PushAlert; seq: bigint } | null {
  try {
    const { seq, alert } = openPushPayload(secret.pid, fromB64url(secret.key), payload);
    if (seq <= BigInt(secret.lastSeq)) return null;
    return { alert, seq };
  } catch {
    return null;
  }
}
