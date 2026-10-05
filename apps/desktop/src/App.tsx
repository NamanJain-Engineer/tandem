import { lazy, Suspense, useEffect, useState } from "react";
import type { AppBoot, AuthState, TermColors } from "@tandem/protocol";
import {
  UiProvider,
  terminalThemeFor,
  useTheme,
  useToast,
  type Density,
  type MotionSetting,
  type TerminalThemeSetting,
  type TextSize,
  type ThemeSetting,
} from "@tandem/ui";
import { AppShell } from "./features/shell/AppShell";
import { DeepLinks } from "./features/shell/DeepLinks";
import { useBoot } from "./features/shell/useBoot";
import { SignIn } from "./features/signin/SignIn";
import { Splash } from "./features/splash/Splash";
import { t } from "./i18n";
import { inTauri, invoke } from "./ipc/invoke";
import { installGlobalErrorLogging, log } from "./ipc/log";
import { useSetting, useSettings, watchSettings } from "./stores/settings";
import { canUseApp, useSession, watchSession } from "./stores/session";
import { useUi, type Mode } from "./stores/ui";
import { useWorkspaces } from "./stores/workspaces";

const Gallery = lazy(() => import("@tandem/ui/gallery").then((m) => ({ default: m.Gallery })));

/** Tells the core which colours to answer OSC 10/11/12 with (F-SH-05 AC2). */
export function TerminalColorsSync() {
  const { resolved, highContrast } = useTheme();
  const setting = useSetting<TerminalThemeSetting>("terminal_theme", "follow_app");
  useEffect(() => {
    const theme = terminalThemeFor(setting, resolved, highContrast);
    const colors: TermColors = { fg: theme.foreground, bg: theme.background, cursor: theme.cursor };
    invoke("term_set_theme_colors", { colors }).catch(() => undefined);
  }, [setting, resolved, highContrast]);
  return null;
}

/** "Signed in as {email}" once, when sign-in completes. */
function SignInToast() {
  const auth = useSession((s) => s.auth);
  const { show } = useToast();
  const [prev, setPrev] = useState<AuthState | null>(null);
  useEffect(() => {
    if (prev && !prev.signedIn && auth?.signedIn && auth.email)
      show({ tone: "success", title: t("signin.signedInAs", { email: auth.email }) });
    setPrev(auth);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [auth]);
  return null;
}

export function App() {
  const boot = useBoot((s) => s.boot);
  const setBoot = useBoot((s) => s.setBoot);
  const [ready, setReady] = useState(false);
  const [hash, setHash] = useState(() => window.location.hash);
  const auth = useSession((s) => s.auth);
  const theme = useSetting<ThemeSetting>("theme", "dark");
  const motion = useSetting<MotionSetting>("reduce_motion", "system");
  const textSize = useSetting<TextSize>("interface_text_size", "default");
  const density = useSetting<Density>("density", "compact");

  useEffect(() => {
    const onHash = () => setHash(window.location.hash);
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  useEffect(() => {
    if (!inTauri()) return;
    installGlobalErrorLogging();
    let cleanup: Array<() => void> = [];
    let started = false;
    // Show the window after the first paint (ARCHITECTURE §1.3 step 5). A
    // hidden window gets no animation frames in some webviews, so a timer
    // backs the frame callback up.
    const start = () => {
      if (started) return;
      started = true;
      void (async () => {
        try {
          const b = await invoke<AppBoot>("app_ready");
          setBoot(b);
          useUi.getState().setMode((b.lastMode as Mode) ?? "code");
          await Promise.all([
            useSettings.getState().load(),
            useSession.getState().load(),
            useWorkspaces.getState().refresh(),
          ]);
          cleanup = [await watchSettings(), await watchSession()];
          invoke("telemetry_capture", {
            event: "app_started",
            props: { platform: b.platform, version: b.version, first_run: b.firstRun },
          }).catch(() => undefined);
          setReady(true);
        } catch (e) {
          log("error", "ui.boot_failed", { code: (e as { code?: string }).code ?? "internal" });
          setReady(true);
        }
      })();
    };
    const frame = requestAnimationFrame(start);
    const timer = setTimeout(start, 50);
    return () => {
      cancelAnimationFrame(frame);
      clearTimeout(timer);
      cleanup.forEach((f) => f());
    };
  }, [setBoot]);

  // Refresh workspaces when the session changes (plan → limits).
  useEffect(() => {
    if (ready) void useWorkspaces.getState().refresh();
  }, [ready, auth?.signedIn]);

  if (boot?.dev && hash === "#/__gallery") {
    return (
      <Suspense fallback={<Splash />}>
        <Gallery />
      </Suspense>
    );
  }

  return (
    <UiProvider theme={theme} motion={motion} textSize={textSize} density={density}>
      {!ready || !auth ? (
        <Splash />
      ) : (
        <>
          <TerminalColorsSync />
          <SignInToast />
          {canUseApp(auth) ? (
            <>
              <DeepLinks />
              <AppShell />
            </>
          ) : (
            <SignIn />
          )}
        </>
      )}
    </UiProvider>
  );
}
