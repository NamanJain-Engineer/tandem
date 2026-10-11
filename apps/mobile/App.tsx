// Vibezeo Remote (F-RM-05): pairs with the person's computer by QR code or
// link (hosted relay or LAN mode), then shows its agents, approvals, threads,
// teammate chats and routines over the end-to-end encrypted protocol
// (@tandem/remote). Keys live in the Keychain / Keystore.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AppState, Linking, Platform, StyleSheet, Text, View } from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { StatusBar } from "expo-status-bar";
import Constants from "expo-constants";
import * as Device from "expo-device";
import * as Notifications from "expo-notifications";
import * as SecureStore from "expo-secure-store";
import { DemoDesktop } from "@tandem/remote/demo";
import { RemoteSession, type Notice } from "@tandem/remote/session";
import { MemoryPairingStore } from "@tandem/remote/store";
import { NoticeGate, present, routeNotice } from "./lib/notify";
import { DEFAULT_PREFS, loadPrefs, savePrefs, type Prefs } from "./lib/prefs";
import { pushAvailable, registerPushTask, syncPush } from "./lib/push";
import { SecureStorePairingStore } from "./lib/secureStore";
import { t } from "./lib/strings";
import { useSessionView } from "./lib/useSession";
import { Home } from "./screens/Home";
import { Scan } from "./screens/Scan";
import { Welcome } from "./screens/Welcome";
import { color, space } from "./ui/tokens";

const POLL_MS = 10_000;
const BACKGROUND_GRACE_MS = 60_000;
const SECURE = { keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY };

registerPushTask();

Notifications.setNotificationHandler({
  handleNotification: async () => ({
    shouldShowBanner: true,
    shouldShowList: true,
    shouldPlaySound: false,
    shouldSetBadge: false,
  }),
});

function deviceLabel(): string {
  const model = Device.modelName ?? (Platform.OS === "ios" ? "iPhone" : "Android");
  return `${model} · ${t("app.name")}`;
}

const secureKv = {
  getItemAsync: (k: string) => SecureStore.getItemAsync(k, SECURE),
  setItemAsync: (k: string, v: string) => SecureStore.setItemAsync(k, v, SECURE),
  deleteItemAsync: (k: string) => SecureStore.deleteItemAsync(k, SECURE),
};

export default function App() {
  const [prefs, setPrefs] = useState<Prefs>(DEFAULT_PREFS);
  const prefsRef = useRef(prefs);
  prefsRef.current = prefs;
  const [banner, setBanner] = useState<{ title: string; body: string } | null>(null);
  const [scanning, setScanning] = useState(false);
  const [demo, setDemo] = useState<DemoDesktop | null>(null);
  const gate = useRef(new NoticeGate());
  const phase = useRef(AppState.currentState);

  const onNotice = useCallback((notice: Notice) => {
    if (!gate.current.admit(notice)) return;
    const shown = present(notice);
    const route = routeNotice(prefsRef.current, phase.current === "active" ? "active" : "background");
    if (route === "banner") setBanner(shown);
    if (route === "os") {
      void Notifications.scheduleNotificationAsync({
        identifier: shown.id,
        content: { title: shown.title, body: shown.body },
        trigger: null,
      }).catch(() => undefined);
    }
  }, []);

  const real = useMemo(
    () =>
      new RemoteSession(new SecureStorePairingStore(secureKv), {
        device: deviceLabel(),
        onNotice,
      }),
    [onNotice],
  );
  const demoSession = useMemo(
    () =>
      demo
        ? new RemoteSession(new MemoryPairingStore(), { socketFactory: demo.socketFactory(), demo: true, device: deviceLabel(), onNotice })
        : null,
    [demo, onNotice],
  );
  const session = demoSession ?? real;
  const view = useSessionView(session);

  useEffect(() => {
    void loadPrefs(secureKv).then(setPrefs);
    void real.start();
    // Pairing links opened from the camera app, a browser or a message.
    const open = (url: string | null) => {
      if (url && /[#?]p=/.test(url)) {
        setDemo(null);
        setScanning(false);
        void real.pairUrl(url);
      }
    };
    void Linking.getInitialURL().then(open);
    const sub = Linking.addEventListener("url", (e) => open(e.url));
    return () => {
      sub.remove();
      real.stop();
    };
  }, [real]);

  useEffect(() => {
    if (!demoSession || !demo) return;
    void demoSession.start().then(() => demoSession.pair(demo.link()));
    return () => demoSession.stop();
  }, [demo, demoSession]);

  // Foreground: reconnect and refresh. Background: let go after a grace period.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const sub = AppState.addEventListener("change", (next) => {
      phase.current = next;
      if (timer) clearTimeout(timer);
      if (next === "active") {
        session.resume();
        void session.refresh();
      } else timer = setTimeout(() => session.stop(), BACKGROUND_GRACE_MS);
    });
    return () => {
      sub.remove();
      if (timer) clearTimeout(timer);
    };
  }, [session]);

  useEffect(() => {
    if (view.phase.kind !== "paired" || view.connection !== "online") return;
    const timer = setInterval(() => {
      if (phase.current === "active") void session.refresh();
    }, POLL_MS);
    return () => clearInterval(timer);
  }, [session, view.phase.kind, view.connection]);

  useEffect(() => {
    if (!banner) return;
    const timer = setTimeout(() => setBanner(null), 5000);
    return () => clearTimeout(timer);
  }, [banner]);

  async function changePrefs(next: Prefs) {
    if (next.notifications && !prefs.notifications) await Notifications.requestPermissionsAsync().catch(() => undefined);
    setPrefs(next);
    await savePrefs(secureKv, next);
    if (!demo) await syncPush(real, next).catch(() => undefined);
  }

  useEffect(() => {
    // Ask once after pairing, when notifications are on (Android 13+, iOS).
    if (view.phase.kind === "paired" && prefs.notifications && !demo) {
      void Notifications.requestPermissionsAsync().catch(() => undefined);
    }
  }, [view.phase.kind, prefs.notifications, demo]);

  const version = Constants.expoConfig?.version ?? "0.0.0";
  let body;
  if (scanning) {
    body = (
      <Scan
        onCancel={() => setScanning(false)}
        onCode={(data) => {
          setScanning(false);
          void real.pairUrl(data);
        }}
      />
    );
  } else if (view.phase.kind === "paired") {
    body = (
      <Home
        session={session}
        view={view}
        demo={!!demo}
        prefs={prefs}
        pushAvailable={pushAvailable()}
        version={version}
        onPrefs={(p) => void changePrefs(p)}
        onLeaveDemo={() => setDemo(null)}
        onDemoNext={() => demo?.advance()}
      />
    );
  } else {
    body = (
      <Welcome
        phase={view.phase}
        onScan={() => setScanning(true)}
        onPaste={(url) => void real.pairUrl(url)}
        onDemo={() => setDemo(new DemoDesktop())}
      />
    );
  }

  return (
    <SafeAreaProvider>
      <SafeAreaView style={st.root} edges={["top", "bottom"]}>
        <StatusBar style="light" />
        {body}
        {banner && (
          <View style={st.banner} accessibilityRole="alert">
            <Text style={st.bannerTitle}>{banner.title}</Text>
            {banner.body ? <Text style={st.bannerBody}>{banner.body}</Text> : null}
          </View>
        )}
      </SafeAreaView>
    </SafeAreaProvider>
  );
}

const st = StyleSheet.create({
  root: { flex: 1, backgroundColor: color.bg },
  banner: {
    position: "absolute",
    top: space[8],
    left: space[4],
    right: space[4],
    padding: space[3],
    borderRadius: 8,
    backgroundColor: color.surface3,
    borderLeftWidth: 3,
    borderLeftColor: color.accent,
  },
  bannerTitle: { color: color.text, fontWeight: "600" },
  bannerBody: { color: color.textMuted, marginTop: 2 },
});
