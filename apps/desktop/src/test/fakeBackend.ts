import { resetDashboard } from "../stores/dashboard";
import { useInstall } from "../features/code/InstallSheet";
import { resetSidebar } from "../stores/sidebar";
import { useEngines } from "../stores/engines";
import type { AppBoot, AuthState, EntitlementView, Limits, WorkspaceSummary } from "@tandem/protocol";
import { setInvokeBackend } from "../ipc/invoke";
import { useBoot } from "../features/shell/useBoot";
import { useSession } from "../stores/session";
import { useSettings } from "../stores/settings";
import { useUi } from "../stores/ui";
import { useWorkspaces } from "../stores/workspaces";

export const FREE: Limits = {
  panes: 4,
  workspaces: 3,
  threadGrid: 1,
  teammates: 1,
  routines: false,
  backgroundRoutines: false,
  taskExecution: false,
  pluginConnections: 1,
  accountsPerEngine: 1,
  cloudStt: false,
  enhance: false,
  conductor: false,
  auto: false,
  phone: false,
  admin: false,
  chatEngines: ["claude", "codex"],
  creditPacks: false,
};

export const BOOT: AppBoot = {
  version: "0.1.0",
  build: "dev",
  platform: "linux",
  dev: true,
  firstRun: false,
  lastMode: "code",
  flags: [],
  dataDirDisplay: "~/.tandem",
};

export const SIGNED_IN: AuthState = {
  signedIn: true,
  email: "ana@tandem.test",
  offline: false,
  offlineDaysLeft: null,
  pending: null,
  error: null,
};

export const FREE_VIEW: EntitlementView = {
  plan: "free",
  status: "none",
  limits: FREE,
  source: "token",
  credits: { monthly: 0, purchased: 0, resetsAt: null },
  policy: null,
  paidUntil: null,
  cancelAtPeriodEnd: false,
  online: true,
  confirmedUntil: null,
  monthlyGrant: 10000,
};

export type Call = { cmd: string; args?: Record<string, unknown> };

/** A scripted core: records every call and answers from `handlers`. */
export function fakeBackend(
  handlers: Record<string, (args?: Record<string, unknown>) => unknown> = {},
): Call[] {
  const calls: Call[] = [];
  const settings: Record<string, unknown> = {};
  const defaults: Record<string, (args?: Record<string, unknown>) => unknown> = {
    settings_get_all: () => settings,
    settings_set: (a) => {
      settings[String(a?.key)] = a?.value;
      return null;
    },
    workspace_list: () => [] as WorkspaceSummary[],
    deeplink_pending: () => [],
    auth_state: () => SIGNED_IN,
    entitlement_get: () => FREE_VIEW,
    telemetry_capture: () => null,
    term_set_theme_colors: () => null,
    app_log: () => null,
  };
  setInvokeBackend(async (cmd, args) => {
    calls.push({ cmd, args });
    const h = handlers[cmd] ?? defaults[cmd];
    if (!h) return null;
    return h(args);
  });
  return calls;
}

/** Resets the stores to a signed-in Free session on Linux. */
export function resetStores(): void {
  resetSidebar();
  resetDashboard();
  useInstall.setState({ engine: null, tabId: null });
  useEngines.setState({ list: [], loaded: false, report: null, detecting: false });
  useBoot.setState({ boot: BOOT });
  useSession.setState({ auth: SIGNED_IN, entitlement: FREE_VIEW });
  useSettings.setState({ values: {}, loaded: true });
  useUi.setState({
    mode: "code",
    settingsOpen: false,
    returnFocus: null,
    railCollapsed: false,
    paletteOpen: false,
    upgrade: { open: false },
    settingsPage: null,
  });
  useWorkspaces.setState({ list: [], activeId: null, activeTab: {} });
}
