import { LaunchSheet } from "../code/LaunchSheet";
import { HandoffDialog } from "../code/HandoffDialog";
import { AutoDialog } from "../auto/AutoDialog";
import { followAuto } from "../../stores/auto";
import { WorktreeDialogs } from "../code/Worktrees";
import { useEffect } from "react";
import { useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useLayout } from "../../stores/layout";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { UpgradeDialog } from "../billing/UpgradeDialog";
import { WhatsNew } from "../whatsnew/WhatsNew";
import { CodeMain } from "../code/CodeMain";
import { CodeRail } from "../code/CodeRail";
import { commandRuns, setCommandToast } from "../commands/commands";
import { InterimMain, InterimRail } from "../interim/Interim";
import { ThreadMain } from "../thread/ThreadMain";
import { ThreadRail } from "../thread/ThreadRail";
import { followThreads } from "../../stores/threads";
import { Palette } from "../palette/Palette";
import { Cheatsheet } from "../commands/Cheatsheet";
import { OrphansDialog } from "../restore/OrphansDialog";
import { followEngines } from "../../stores/engines";
import { followLimits } from "../../stores/limits";
import { followContexts } from "../../stores/contexts";
import { followUsage } from "../../stores/usage";
import { followStatus } from "../../stores/status";
import { followGit } from "../../stores/git";
import { followBrowser } from "../../stores/browser";
import { followBrowserTabs } from "../../stores/sidebar";
import { followFocus } from "../../stores/focus";
import { followDashboard } from "../../stores/dashboard";
import { followNotifications } from "../../stores/notifications";
import { followRestore, startAutoResume } from "../../stores/restore";
import { followUpdates } from "../../stores/updates";
import { RestartDialog } from "../updates/RestartDialog";
import { UnsupportedOverlay } from "../updates/UnsupportedOverlay";
import { useVisibilityReport } from "../notifications/visibility";
import { InstallHost } from "../code/InstallSheet";
import { followPtyState } from "../../stores/terminals";
import { terminalHandles } from "../code/TerminalView";
import { enablePerfMarks, perfSpans, perfTimes, reflowCounts, writeCounts } from "../code/perfMarks";
import { webglPool } from "../code/webglPool";
import { useBoot } from "./useBoot";
import { SettingsView } from "../settings/SettingsView";
import { Banners } from "./Banners";
import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";
import { useShortcuts } from "./useShortcuts";

/** UI §3.1 window anatomy. Mode surfaces stay mounted (hidden with
 * display:none) so terminals never re-mount (§3.4). */
export function AppShell() {
  const { mode, settingsOpen, railCollapsed } = useUi();
  const { show: toast } = useToast();
  useShortcuts();
  useEffect(() => setCommandToast(toast), [toast]);
  useEffect(followPtyState, []);
  useEffect(followEngines, []);
  useEffect(followLimits, []);
  useEffect(followAuto, []);
  useEffect(followContexts, []);
  useEffect(followUsage, []);
  useEffect(followStatus, []);
  useEffect(followGit, []);
  useEffect(followBrowser, []);
  useEffect(followBrowserTabs, []);
  useEffect(followFocus, []);
  useEffect(followDashboard, []);
  useEffect(followNotifications, []);
  useEffect(followRestore, []);
  useEffect(startAutoResume, []);
  useEffect(followUpdates, []);
  useEffect(followThreads, []);
  useVisibilityReport();
  // Debug builds: UI-side counters for the E2E harness (ARCHITECTURE §17.5).
  const dev = useBoot((st) => st.boot?.dev ?? false);
  useEffect(() => {
    if (!dev) return;
    (window as unknown as { __tandemTest?: object }).__tandemTest = {
      webglContexts: () => webglPool.liveContexts(),
      paneText: (paneId: string) => terminalHandles.get(paneId)?.text() ?? "",
      selectAll: (paneId: string) => terminalHandles.get(paneId)?.selectAll(),
      paneScreen: (paneId: string) => terminalHandles.get(paneId)?.serialize() ?? "",
      paneCursor: (paneId: string) => terminalHandles.get(paneId)?.cursor() ?? null,
      paneMetrics: (paneId: string) => terminalHandles.get(paneId)?.metrics() ?? null,
      panePoint: (paneId: string, needle: string) => terminalHandles.get(paneId)?.pointAt(needle) ?? null,
      webglHolds: (paneId: string) => webglPool.holds(paneId),
      perf: { enable: enablePerfMarks, times: perfTimes, writes: writeCounts, spans: perfSpans },
      paneSize: (paneId: string) => terminalHandles.get(paneId)?.size() ?? null,
      reflows: (reset?: boolean) => reflowCounts(reset ?? false),
      commandRuns: () => Object.fromEntries(commandRuns),
      // State the harness saves next to a failure's screenshot.
      debug: () => {
        const ws = useWorkspaces.getState();
        const layout = useLayout.getState();
        const active = document.activeElement;
        return {
          mode: useUi.getState().mode,
          handles: [...terminalHandles.keys()],
          domPanes: Array.from(document.querySelectorAll<HTMLElement>('[data-testid="pane"]')).map((el) => el.dataset.paneId),
          activeWorkspace: ws.activeId,
          activeTab: ws.activeTab,
          layoutTabs: Object.fromEntries(Object.entries(layout.tabs).map(([id, tab]) => [id, { active: tab.active, panes: Object.keys(tab.panes) }])),
          picker: layout.picker,
          activeElement: active ? `${active.tagName}.${String(active.className).slice(0, 60)}` : null,
          dialogs: Array.from(document.querySelectorAll("[role=dialog],[role=alertdialog]")).map((el) => el.textContent?.slice(0, 80)),
        };
      },
    };
  }, [dev]);
  const show = (m: typeof mode) => (!settingsOpen && mode === m ? "flex" : "none");
  return (
    <div className="flex h-full flex-col bg-bg">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:left-2 focus:top-2 focus:z-[var(--z-max)] focus:rounded-sm focus:bg-surface-3 focus:px-3 focus:py-1.5 focus:text-fg"
      >
        {t("app.skipToMain")}
      </a>
      <TitleBar />
      <Banners />
      {/* relative: a floating or full-screen sidebar overlays this row. */}
      <div className="relative flex min-h-0 flex-1">
        {settingsOpen && <SettingsView />}
        <InstallHost />
        <div style={{ display: show("code") }}>
          <CodeRail />
        </div>
        <div style={{ display: show("agent") }}>
          <InterimRail mode="agent" collapsed={railCollapsed} />
        </div>
        <div style={{ display: show("thread") }}>
          <ThreadRail collapsed={railCollapsed} />
        </div>
        <main
          id={settingsOpen ? undefined : "main"}
          tabIndex={-1}
          className="min-w-0 flex-1 p-1.5 focus:outline-none"
          style={{ display: settingsOpen ? "none" : "block" }}
        >
          <div className="h-full" style={{ display: mode === "code" ? "block" : "none" }}>
            <CodeMain visible={!settingsOpen && mode === "code"} />
          </div>
          {/* Every surface stays mounted; hidden ones are display:none (F-SH-02). */}
          <div className="h-full" style={{ display: mode === "agent" ? "block" : "none" }}>
            <InterimMain mode="agent" />
          </div>
          <div className="h-full" style={{ display: mode === "thread" ? "block" : "none" }}>
            <ThreadMain visible={!settingsOpen && mode === "thread"} />
          </div>
        </main>
        {!settingsOpen && <Sidebar />}
      </div>
      <StatusBar />
      <UpgradeDialog />
      <WhatsNew />
      <WorktreeDialogs />
      <LaunchSheet />
      <HandoffDialog />
      <AutoDialog />
      <Palette />
      <Cheatsheet />
      <OrphansDialog />
      <RestartDialog />
      <UnsupportedOverlay />
    </div>
  );
}
