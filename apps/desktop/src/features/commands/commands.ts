// The command registry's handlers (F-SH-09). Metadata (ids, bindings,
// predicates) comes from `tandem-core` through
// `@tandem/protocol/shortcuts.json`; the palette, the shortcut dispatcher and
// menus all run commands through `runCommand`, so they share one handler.
import type { CommandSpec } from "@tandem/protocol";
import shortcuts from "@tandem/protocol/shortcuts.json";
import { t, type MessageKey } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { applyDrop } from "../code/dnd";
import type { Side } from "../code/layout/tree";
import { terminalHandles } from "../code/TerminalView";
import { copyDiagnostics, pickAndAddWorkspace, switchMode } from "../shell/actions";

export const COMMANDS = shortcuts as CommandSpec[];

type Toast = (t: { tone?: "info" | "success" | "warning" | "error"; title: string; body?: string }) => void;
let toast: Toast = () => undefined;

/** The app's toast function, for commands run outside React. */
export function setCommandToast(fn: Toast): void {
  toast = fn;
}

export function commandTitle(id: string): string {
  return t(`cmd.${id}` as MessageKey);
}

/** The binding in effect for this platform and focus. */
export function bindingFor(spec: CommandSpec, ctx: { mac: boolean; terminalFirst: boolean }): string | null {
  if (ctx.mac) return spec.mac;
  return ctx.terminalFirst ? spec.pcTerminal : spec.pc;
}

function activeTab(): string | null {
  const ws = useWorkspaces.getState();
  return ws.activeId ? (ws.activeTab[ws.activeId] ?? null) : null;
}

function activePane(): { tabId: string; paneId: string } | null {
  const tabId = activeTab();
  const paneId = tabId ? useLayout.getState().tabs[tabId]?.active : null;
  return tabId && paneId ? { tabId, paneId } : null;
}

export function isEnabled(spec: CommandSpec): boolean {
  if (spec.enabledWhen === "always") return true;
  const ui = useUi.getState();
  if (ui.mode !== "code" || ui.settingsOpen) return false;
  if (spec.enabledWhen === "code_mode") return true;
  const ws = useWorkspaces.getState();
  const active = ws.list.find((w) => w.id === ws.activeId);
  if (!active || active.missing || !activeTab()) return false;
  if (spec.enabledWhen === "workspace_open") return true;
  return activePane() !== null;
}

function split(side: Side): void {
  const tabId = activeTab();
  if (tabId && useLayout.getState().requestSplit(tabId, null, side) === "no_room")
    toast({ tone: "warning", title: t("split.noRoom") });
}

function cycleTab(delta: 1 | -1): void {
  const ws = useWorkspaces.getState();
  const active = ws.list.find((w) => w.id === ws.activeId);
  const current = activeTab();
  if (!active || active.tabs.length < 2) return;
  const i = active.tabs.findIndex((x) => x.id === current);
  const next = active.tabs[(i + delta + active.tabs.length) % active.tabs.length];
  if (next) ws.selectTab(active.id, next.id);
}

function onPane(f: (tabId: string, paneId: string) => void): () => void {
  return () => {
    const p = activePane();
    if (p) f(p.tabId, p.paneId);
  };
}

/** Every registry command's handler. A registry id without one fails the
 * registry test, so the palette never lists a dead command. */
export const handlers: Record<string, () => void> = {
  "palette.open": () => useUi.getState().setPalette(!useUi.getState().paletteOpen),
  "settings.open": () => useUi.getState().openSettings(),
  "mode.agent": () => switchMode("agent"),
  "mode.code": () => switchMode("code"),
  "mode.thread": () => switchMode("thread"),
  "rail.toggle": () => useUi.getState().toggleRail(),
  "sidebar.toggle": () => useUi.getState().toggleSidebar(),
  "workspace.open_folder": () => void pickAndAddWorkspace(toast),
  "workspace.new_tab": () => {
    const ws = useWorkspaces.getState();
    if (ws.activeId) void ws.createTab(ws.activeId);
  },
  "workspace.next_tab": () => cycleTab(1),
  "workspace.prev_tab": () => cycleTab(-1),
  "workspace.arrange": () => {
    const tabId = activeTab();
    if (tabId) useLayout.getState().arrange(tabId);
  },
  "pane.split_right": () => split("right"),
  "pane.split_down": () => split("bottom"),
  "pane.close": onPane((tabId, paneId) => void useLayout.getState().close(tabId, paneId)),
  "pane.focus_left": onPane((tabId) => useLayout.getState().focusDirection(tabId, "left")),
  "pane.focus_right": onPane((tabId) => useLayout.getState().focusDirection(tabId, "right")),
  "pane.focus_up": onPane((tabId) => useLayout.getState().focusDirection(tabId, "top")),
  "pane.focus_down": onPane((tabId) => useLayout.getState().focusDirection(tabId, "bottom")),
  "pane.focus_prev": onPane((tabId) => useLayout.getState().focusCycle(tabId, -1)),
  "pane.focus_next": onPane((tabId) => useLayout.getState().focusCycle(tabId, 1)),
  "pane.maximize": onPane((tabId, paneId) => useLayout.getState().toggleMaximize(tabId, paneId)),
  "pane.move_to_new_tab": onPane((tabId, paneId) => {
    const ws = useWorkspaces.getState().activeId;
    if (ws) void applyDrop({ kind: "pane", paneId, tabId, title: "" }, { kind: "new_tab", workspaceId: ws });
  }),
  "pane.restart": onPane((_, paneId) => terminalHandles.get(paneId)?.restart()),
  "billing.plans": () => useUi.getState().openUpgrade(),
  "help.copy_diagnostics": () => void copyDiagnostics(toast),
  "help.open_logs": () => void invoke("app_open_logs_folder").catch(() => undefined),
};

/** Runs a command if it's enabled. Returns whether it ran. */
export function runCommand(id: string): boolean {
  const spec = COMMANDS.find((c) => c.id === id);
  const handler = handlers[id];
  if (!spec || !handler || !isEnabled(spec)) return false;
  handler();
  return true;
}
