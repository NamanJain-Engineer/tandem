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
import { Splash, type SplashPhase } from "./features/splash/Splash";
import { t } from "./i18n";
import { inTauri, invoke, usingFakeBackend } from "./ipc/invoke";
import { installGlobalErrorLogging, log } from "./ipc/log";
import { useSetting, useSettings, watchSettings } from "./stores/settings";
import { canUseApp, useSession, watchSession } from "./stores/session";
import { useUi, type Mode } from "./stores/ui";
import { useWorkspaces } from "./stores/workspaces";
import { useRestore } from "./stores/restore";
import { useOnboarding } from "./stores/onboarding";
import { Onboarding } from "./features/onboarding/Onboarding";

/** The plan check holds the splash at most this long, then the app
 * continues from the cached entitlement (F-SH-07). */
const PLAN_WAIT_MS = 3_000;

/** Resolves when `work` settles, after `ms`, or when `skip` fires. */
function bounded(work: Promise<unknown>, ms: number, skip: Promise<void>): Promise<void> {
  return Promise.race([work.then(() => undefined, () => undefined), new Promise<void>((r) => setTimeout(r, ms)), skip]);
}

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
  const [splashPhase, setSplashPhase] = useState<SplashPhase>("starting");
  const [skipPlan] = useState(() => {
    let fire = () => undefined as void;
    const promise = new Promise<void>((r) => (fire = r));
    return { promise, fire: () => fire() };
  });
  const [hash, setHash] = useState(() => window.location.hash);
  const auth = useSession((s) => s.auth);
  const theme = useSetting<ThemeSetting>("theme", "dark");
  const motion = useSetting<MotionSetting>("reduce_motion", "system");
  const textSize = useSetting<TextSize>("interface_text_size", "default");
  const density = useSetting<Density>("density", "compact");
  const onboardingDone = useOnboarding((s) => s.state?.done ?? true);

  useEffect(() => {
    const onHash = () => setHash(window.location.hash);
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  useEffect(() => {
    // The browser demo (vibezeo.com/demo) boots against its simulated core.
    if (!inTauri() && !usingFakeBackend()) return;
    if (inTauri()) installGlobalErrorLogging();
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
          // "Restoring your session…", then "Checking your plan…" (F-SH-07).
          setSplashPhase("restoring");
          const session = useSession.getState().load();
          await Promise.all([
            useSettings.getState().load(),
            useWorkspaces.getState().refresh(),
            useRestore.getState().load(),
            useOnboarding.getState().load(),
          ]);
          setSplashPhase("plan");
          await bounded(session, PLAN_WAIT_MS, skipPlan.promise);
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
  }, [setBoot, skipPlan]);

  // Any key or click while the splash shows skips the wait.
  useEffect(() => {
    if (ready) return;
    const skip = () => skipPlan.fire();
    window.addEventListener("keydown", skip, true);
    window.addEventListener("pointerdown", skip, true);
    return () => {
      window.removeEventListener("keydown", skip, true);
      window.removeEventListener("pointerdown", skip, true);
    };
  }, [ready, skipPlan]);

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
        <Splash phase={splashPhase} onAnyInput={skipPlan.fire} />
      ) : (
        <>
          <TerminalColorsSync />
          <SignInToast />
          {canUseApp(auth) ? (
            onboardingDone ? (
              <>
                <DeepLinks />
                <AppShell />
              </>
            ) : (
              // UI-04 after Welcome (the sign-in screen), until done or skipped.
              <Onboarding />
            )
          ) : (
            <SignIn />
          )}
        </>
      )}
    </UiProvider>
  );
}
