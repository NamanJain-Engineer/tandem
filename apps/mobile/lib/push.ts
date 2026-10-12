// Push notifications (F-RM-06, D-2020–D-2023): off until the person turns
// them on, and only for a pairing through the hosted relay. The app gives the
// desktop its push token and a fresh alert key over the encrypted channel;
// the desktop seals each alert with that key and the API forwards bytes it
// can't read.
// - Android: FCM delivers a data-only message; the background task below
//   opens it and shows a local notification.
// - iOS: APNs delivers a generic alert; the Notification Service Extension
//   (plugins/withPushExtension) opens the payload with the key it reads from
//   the shared keychain group and replaces the text before display.
import { Platform } from "react-native";
import Constants from "expo-constants";
import * as Notifications from "expo-notifications";
import * as SecureStore from "expo-secure-store";
import * as TaskManager from "expo-task-manager";
import { randomBytes } from "@noble/ciphers/utils.js";
import type { RemoteSession } from "@tandem/remote/session";
import type { Prefs } from "./prefs";
import { clearPushSecret, loadPushSecret, newPushSecret, openAlert, payloadOf, savePushSecret } from "./pushCore";

export const PUSH_TASK = "vibezeo-push";

type Extra = { pushAndroid?: boolean; pushIos?: boolean; keychainGroup?: string };
const extra = (Constants.expoConfig?.extra ?? {}) as Extra;

/** Readable by the iOS extension while the phone is locked, never synced or backed up. */
function secureOptions(): SecureStore.SecureStoreOptions {
  const base: SecureStore.SecureStoreOptions = {
    keychainService: "vibezeo.push",
    keychainAccessible: SecureStore.AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY,
  };
  return Platform.OS === "ios" && extra.keychainGroup ? { ...base, accessGroup: extra.keychainGroup } : base;
}

const kv = {
  getItemAsync: (k: string, o?: object) => SecureStore.getItemAsync(k, o),
  setItemAsync: (k: string, v: string, o?: object) => SecureStore.setItemAsync(k, v, o),
  deleteItemAsync: (k: string, o?: object) => SecureStore.deleteItemAsync(k, o),
};

/** Whether this build was made with push credentials (Firebase on Android, APNs on iOS). */
export function pushAvailable(): boolean {
  return Platform.OS === "android" ? !!extra.pushAndroid : Platform.OS === "ios" ? !!extra.pushIos && !!extra.keychainGroup : false;
}

/** Turns push on or off for the current pairing, as the prefs say. */
export async function syncPush(session: RemoteSession, prefs: Prefs): Promise<void> {
  const pairing = session.pairing;
  const want = prefs.push && pushAvailable() && !!pairing && pairing.relay.startsWith("wss://");
  if (!want) {
    if (await loadPushSecret(kv, secureOptions())) {
      await session.unregisterPush().catch(() => undefined);
      await clearPushSecret(kv, secureOptions());
    }
    return;
  }
  const perm = await Notifications.requestPermissionsAsync();
  if (!perm.granted) return;
  const token = await Notifications.getDevicePushTokenAsync();
  const secret = newPushSecret(pairing.pid, (n) => randomBytes(n));
  await savePushSecret(kv, secret, secureOptions());
  const r = await session.registerPush(Platform.OS === "ios" ? "apns" : "fcm", String(token.data), secret.key);
  if (!r.ok) await clearPushSecret(kv, secureOptions());
}

/** Opens an alert and shows it (Android, in the background or the foreground). */
export async function showSealed(data: unknown): Promise<boolean> {
  const payload = payloadOf(data);
  const secret = payload ? await loadPushSecret(kv, secureOptions()) : null;
  const opened = payload && secret ? openAlert(secret, payload) : null;
  if (!opened || !secret) return false;
  await savePushSecret(kv, { ...secret, lastSeq: opened.seq.toString() }, secureOptions());
  await Notifications.scheduleNotificationAsync({
    identifier: `${opened.alert.kind}:${opened.alert.id}`,
    content: { title: opened.alert.title, body: opened.alert.body },
    trigger: null,
  });
  return true;
}

// Android delivers data-only messages here even when the app isn't running.
if (Platform.OS === "android") {
  TaskManager.defineTask(PUSH_TASK, async ({ data, error }) => {
    if (error) return;
    await showSealed(data).catch(() => undefined);
  });
}

export function registerPushTask(): void {
  if (Platform.OS === "android" && pushAvailable()) {
    void Notifications.registerTaskAsync(PUSH_TASK).catch(() => undefined);
  }
}
