import { resetNotifications } from "../stores/notifications";
import { resetDashboard } from "../stores/dashboard";
import { useInstall } from "../features/code/InstallSheet";
import { resetSidebar } from "../stores/sidebar";
import { resetRestore } from "../stores/restore";
import { resetUpdates } from "../stores/updates";
import { resetOnboarding } from "../stores/onboarding";
import { useEngines } from "../stores/engines";
import { useContexts } from "../stores/contexts";
import { useUsage } from "../stores/usage";
import type { AppBoot, AuthState, EntitlementView, Limits, WorkspaceSummary } from "@tandem/protocol";
import { LIMITS } from "@tandem/protocol/plans";
import { setInvokeBackend } from "../ipc/invoke";
import { useDictate } from "../stores/dictate";
import { useBoot } from "../features/shell/useBoot";
import { useSession } from "../stores/session";
import { useSettings } from "../stores/settings";
import { useUi } from "../stores/ui";
import { resetThreads } from "../stores/threads";
import { resetTeammates } from "../stores/teammates";
import { resetSuggestions } from "../features/agents/Suggestions";
import { resetFiles } from "../stores/files";
import { resetIntel } from "../stores/intel";
import { resetGit } from "../stores/git";
import { resetChecksPolling } from "../stores/worktrees";
import { resetBrowser } from "../stores/browser";
import { resetScratchpad } from "../stores/scratchpad";
import { resetTasks } from "../stores/tasks";
import { resetRoutines } from "../stores/routines";
import { useWorkspaces } from "../stores/workspaces";

/** What runs with no account (D-869, D-2900): the protocol's own `LIMITS.free`. */
export const FREE: Limits = { ...LIMITS.free };

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
  hosted: true,
  pending: null,
  error: null,
};

export const FREE_VIEW: EntitlementView = {
  plan: "free",
  status: "none",
  limits: FREE,
  source: "token",
  credits: { monthly: 0, purchased: 0, resetsAt: null, cap: null, spent: null, grant: null },
  policy: null,
  org: null,
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
    // Settings › Remote renders the phone section beside SSH and WSL (D-1917).
    phone_pairings_list: () => [],
    phone_queue_list: () => [],
    phone_arm_state: () => ({ armedUntil: null }),
    phone_lan_state: () => ({ enabled: false, running: false, port: 47800, addresses: [], error: null }),
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
  resetTasks();
  resetRoutines();
  resetDashboard();
  resetNotifications();
  resetRestore();
  resetUpdates();
  resetOnboarding();
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
    cheatsheetOpen: false,
    settingsPage: null,
  });
  useWorkspaces.setState({ list: [], activeId: null, activeTab: {} });
  useUsage.setState({ byAccount: {} });
  useContexts.setState({ byPane: {} });
  resetThreads();
  resetTeammates();
  resetSuggestions();
  resetFiles();
  resetIntel();
  resetGit();
  resetChecksPolling();
  resetBrowser();
  useDictate.setState({ status: null, models: null, stats: null, take: null, level: 0, historyRev: 0 });
  resetScratchpad();
}
