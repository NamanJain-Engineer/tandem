import { useEffect } from "react";
import { useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useUi } from "../../stores/ui";
import { UpgradeDialog } from "../billing/UpgradeDialog";
import { CodeMain } from "../code/CodeMain";
import { CodeRail } from "../code/CodeRail";
import { setCommandToast } from "../commands/commands";
import { InterimMain, InterimRail } from "../interim/Interim";
import { Palette } from "../palette/Palette";
import { OrphansDialog } from "../restore/OrphansDialog";
import { followPtyState } from "../../stores/terminals";
import { SettingsView } from "../settings/SettingsView";
import { Banners } from "./Banners";
import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";
import { useShortcuts } from "./useShortcuts";

/** UI §3.1 window anatomy. Mode surfaces stay mounted (hidden with
 * display:none) so terminals never re-mount (§3.4). */
export function AppShell() {
  const { mode, settingsOpen, sidebarOpen, railCollapsed } = useUi();
  const { show: toast } = useToast();
  useShortcuts();
  useEffect(() => setCommandToast(toast), [toast]);
  useEffect(followPtyState, []);
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
      <div className="flex min-h-0 flex-1">
        {settingsOpen && <SettingsView />}
        <div style={{ display: show("code") }}>
          <CodeRail />
        </div>
        <div style={{ display: show("agent") }}>
          <InterimRail mode="agent" collapsed={railCollapsed} />
        </div>
        <div style={{ display: show("thread") }}>
          <InterimRail mode="thread" collapsed={railCollapsed} />
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
            <InterimMain mode="thread" />
          </div>
        </main>
        {sidebarOpen && <Sidebar />}
      </div>
      <StatusBar />
      <UpgradeDialog />
      <Palette />
      <OrphansDialog />
    </div>
  );
}
