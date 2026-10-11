import { create } from "zustand";
import type { CallLogEntry, CallView, IpcError, UiAction } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";
import { switchMode } from "../features/shell/actions";
import { place, useLaunch } from "./launch";
import { useLayout } from "./layout";
import { useSession } from "./session";
import { useSidebar } from "./sidebar";
import { useUi } from "./ui";
import type { LaunchedSeat, SidebarTabKind } from "@tandem/protocol";

export const IDLE: CallView = {
  callId: null,
  phase: "idle",
  muted: false,
  utterance: null,
  caption: null,
  chips: [],
  question: null,
  elapsedMs: 0,
  maxSeconds: 0,
  error: null,
  notice: null,
  receipt: null,
};

/** A call is on (the ring toggles it off). */
export function inCall(v: CallView): boolean {
  return !["idle", "ended", "error"].includes(v.phase);
}

/** Conductor (F-CN-01…04): the call as the core reports it, the mic or
 * speech level for the ring, and the call log sheet. */
type ConductorStore = {
  view: CallView;
  level: number;
  logOpen: boolean;
  setView: (v: CallView) => void;
  setLevel: (l: number) => void;
  openLog: (open: boolean) => void;
};

export const useConductor = create<ConductorStore>((set) => ({
  view: IDLE,
  level: 0,
  logOpen: false,
  setView: (view) => set(inCall(view) ? { view } : { view, level: 0 }),
  setLevel: (level) => set({ level }),
  openLog: (logOpen) => set({ logOpen }),
}));

/** Whether this plan has Conductor (the ring shows; the call command works). */
export function hasConductor(): boolean {
  return !!useSession.getState().entitlement?.limits.conductor;
}

function closePane(paneId: string): void {
  const layout = useLayout.getState();
  const tabId = Object.keys(layout.tabs).find((id) => layout.tabs[id]?.panes[paneId]);
  if (tabId) void layout.close(tabId, paneId);
  else void invoke("pane_close", { paneId }).catch(() => undefined);
}

/** Navigation-only tools the core hands to the window (`conductor:ui-action`). */
export function performUiAction({ action, args }: UiAction): void {
  const s = (k: string) => (typeof args[k] === "string" ? (args[k] as string) : "");
  switch (action) {
    case "switch_mode": {
      const mode = s("mode");
      if (mode === "agent" || mode === "code" || mode === "thread") switchMode(mode);
      break;
    }
    case "open_settings":
      useUi.getState().openSettings(s("page") || undefined);
      break;
    case "toggle_sidebar":
      if (s("side") === "left") useUi.getState().toggleRail();
      else useSidebar.getState().toggle();
      break;
    case "open_sidebar_tab":
      useSidebar.getState().openTab(s("tab") as SidebarTabKind);
      break;
    case "open_launch_wizard":
      useLaunch.getState().open(null);
      break;
    case "open_routines":
      switchMode("agent");
      void import("./agentPage").then(({ useAgentPage }) => useAgentPage.getState().show("routines"));
      break;
    case "dismiss":
      useUi.getState().setPalette(false);
      useConductor.getState().openLog(false);
      break;
    case "tidy": {
      void import("../features/commands/commands").then(({ runCommand }) => runCommand("workspace.arrange"));
      break;
    }
    case "flip":
      switchMode(useUi.getState().mode === "code" ? "thread" : "code");
      break;
    case "place_launched": {
      const launched = (args.launched ?? []) as LaunchedSeat[];
      const shared = typeof args.sharedTabId === "string" ? args.sharedTabId : null;
      void place(launched, shared);
      break;
    }
    case "open_teammate":
      switchMode("agent");
      if (s("teammate")) void import("./teammates").then(({ useTeammates }) => useTeammates.getState().select(s("teammate")));
      break;
    case "new_teammate":
      switchMode("agent");
      void import("../features/agents/NewTeammateSheet").then(({ useNewTeammate }) =>
        useNewTeammate.getState().show(true, s("description") || undefined),
      );
      break;
    case "close_pane":
      if (s("pane")) closePane(s("pane"));
      break;
  }
}

/** Keeps the call current: `conductor:state`, `conductor:level`, `conductor:ui-action`. */
export function followConductor(): () => void {
  void invoke<CallView>("conductor_state", {})
    .then((v) => v && useConductor.getState().setView(v))
    .catch(() => undefined);
  const stops = [
    listen<CallView>("conductor:state", (v) => useConductor.getState().setView(v)),
    listen<number>("conductor:level", (l) => useConductor.getState().setLevel(l)),
    listen<UiAction>("conductor:ui-action", performUiAction),
    // The floating ring's Call log button.
    listen<null>("conductor:open-log", () => useConductor.getState().openLog(true)),
  ];
  return () => stops.forEach((p) => void p.then((stop) => stop()));
}

/** Start a call (the ring, the shortcut, the palette), or open the plans on
 * Free. Startup refusals show in the HUD, never silently. */
export async function startCall(pushToTalk = false): Promise<void> {
  if (!hasConductor()) {
    // Says why the plans opened (D-833).
    useUi.getState().openUpgrade({ kind: "conductor", limit: 0 });
    return;
  }
  try {
    const v = await invoke<CallView>("conductor_start_call", { wake: false, pushToTalk });
    if (v) useConductor.getState().setView(v);
  } catch (e) {
    const err = e as IpcError;
    if (err.code === "forbidden_plan") {
      useUi.getState().openUpgrade();
      return;
    }
    useConductor.getState().setView({
      ...IDLE,
      phase: "error",
      error: { code: "connect_failed", message: err.message },
    });
  }
}

export async function endCall(): Promise<void> {
  await invoke("conductor_end_call", {}).catch(() => undefined);
  const v = useConductor.getState().view;
  if (v.phase === "error" || v.phase === "ended") useConductor.getState().setView(IDLE);
}

export function toggleCall(): void {
  if (inCall(useConductor.getState().view)) void endCall();
  else void startCall();
}

export function setMuted(muted: boolean): void {
  void invoke("conductor_mute", { muted }).catch(() => undefined);
}

export async function confirm(requestId: string, yes: boolean): Promise<string | null> {
  try {
    await invoke("conductor_confirm", { requestId, yes });
    return null;
  } catch (e) {
    return (e as IpcError).message;
  }
}

export async function callLog(callId?: string): Promise<CallLogEntry[]> {
  return (await invoke<CallLogEntry[]>("conductor_call_log", { callId: callId ?? null })) ?? [];
}
