// The command registry's handlers (F-SH-09). Metadata (ids, bindings,
// predicates) comes from `tandem-core` through
// `@tandem/protocol/shortcuts.json`; the palette, the shortcut dispatcher and
// menus all run commands through `runCommand`, so they share one handler.
import { useLaunch } from "../../stores/launch";
import { useNotifications } from "../../stores/notifications";
import { useCloseConfirm } from "../../stores/closeConfirm";
import { useSidebar } from "../../stores/sidebar";
import { useRecall } from "../recall/RecallPicker";
import { usePrompts } from "../prompts/PromptPicker";
import { useSettings } from "../../stores/settings";
import type { CommandSpec } from "@tandem/protocol";
import shortcuts from "@tandem/protocol/shortcuts.json";
import { t, type MessageKey } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useThreads } from "../../stores/threads";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { applyDrop } from "../code/dnd";
import type { Side } from "../code/layout/tree";
import { copyLastOutput, terminalHandles } from "../code/TerminalView";
import { copyDiagnostics, pickAndAddWorkspace, switchMode } from "../shell/actions";
import { terminalMayIntercept } from "./keys";

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

/** Rebound shortcuts (F-SH-09 P2): command id → keys, or "" for none. */
export type ShortcutOverrides = Record<string, string>;

export function shortcutOverrides(): ShortcutOverrides {
  const value = useSettings.getState().values.shortcut_overrides;
  return value && typeof value === "object" ? (value as ShortcutOverrides) : {};
}

/** The binding in effect for this platform and focus. A rebound shortcut
 * replaces the default and its terminal alias; under "Terminal first" it
 * fires in a terminal only if it's on the intercept list. */
export function bindingFor(
  spec: CommandSpec,
  ctx: { mac: boolean; terminalFirst: boolean },
  overrides: ShortcutOverrides = shortcutOverrides(),
): string | null {
  const rebound = overrides[spec.id];
  if (typeof rebound === "string") {
    if (!rebound) return null;
    return ctx.mac || !ctx.terminalFirst || terminalMayIntercept(rebound) ? rebound : null;
  }
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
  if (spec.enabledWhen === "thread_open")
    return ui.mode === "thread" && !ui.settingsOpen && useThreads.getState().selectedId !== null;
  if (spec.enabledWhen === "pane_or_cell" && ui.mode === "thread")
    return !ui.settingsOpen && useThreads.getState().grid.length > 1;
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
  "thread.changes": () => useThreads.getState().toggleChanges(),
  "rail.toggle": () => useUi.getState().toggleRail(),
  "sidebar.toggle": () => useSidebar.getState().toggle(),
  "sidebar.files": () => useSidebar.getState().openTab("files"),
  "sidebar.checkpoints": () => useSidebar.getState().openTab("checkpoints"),
  "sidebar.ports": () => useSidebar.getState().openTab("ports"),
  "recall.open": () => useRecall.getState().show(),
  "prompts.open": () => usePrompts.getState().show(),
  "notifications.open": () => useNotifications.getState().setOpen(true),
  "dashboard.full_view": () => {
    const sb = useSidebar.getState();
    sb.openTab("dashboard");
    sb.setFullscreen(!sb.fullscreen);
  },
  "workspace.open_folder": () => void pickAndAddWorkspace(toast),
  "workspace.new_tab": () => {
    const ws = useWorkspaces.getState();
    if (ws.activeId) void ws.createTab(ws.activeId);
  },
  "workspace.launch": () => useLaunch.getState().open(null),
  "workspace.next_tab": () => cycleTab(1),
  "workspace.prev_tab": () => cycleTab(-1),
  "workspace.arrange": () => {
    const tabId = activeTab();
    if (tabId) useLayout.getState().arrange(tabId);
  },
  "pane.split_right": () => split("right"),
  "pane.split_down": () => split("bottom"),
  "pane.close": onPane((tabId, paneId) => useCloseConfirm.getState().request(tabId, paneId)),
  "pane.focus_left": () =>
    useUi.getState().mode === "thread"
      ? useThreads.getState().focusCell("left")
      : onPane((tabId) => useLayout.getState().focusDirection(tabId, "left"))(),
  "pane.focus_right": () =>
    useUi.getState().mode === "thread"
      ? useThreads.getState().focusCell("right")
      : onPane((tabId) => useLayout.getState().focusDirection(tabId, "right"))(),
  "pane.focus_up": () =>
    useUi.getState().mode === "thread"
      ? useThreads.getState().focusCell("up")
      : onPane((tabId) => useLayout.getState().focusDirection(tabId, "top"))(),
  "pane.focus_down": () =>
    useUi.getState().mode === "thread"
      ? useThreads.getState().focusCell("down")
      : onPane((tabId) => useLayout.getState().focusDirection(tabId, "bottom"))(),
  "pane.focus_prev": onPane((tabId) => useLayout.getState().focusCycle(tabId, -1)),
  "pane.focus_next": onPane((tabId) => useLayout.getState().focusCycle(tabId, 1)),
  "pane.maximize": onPane((tabId, paneId) => useLayout.getState().toggleMaximize(tabId, paneId)),
  "pane.move_to_new_tab": onPane((tabId, paneId) => {
    const ws = useWorkspaces.getState().activeId;
    if (ws) void applyDrop({ kind: "pane", paneId, tabId, title: "" }, { kind: "new_tab", workspaceId: ws });
  }),
  "pane.restart": onPane((_, paneId) => terminalHandles.get(paneId)?.restart()),
  "terminal.jump_bottom": onPane((_, paneId) => terminalHandles.get(paneId)?.jumpToBottom()),
  "terminal.find": onPane((_, paneId) => terminalHandles.get(paneId)?.find()),
  "terminal.find_next": onPane((_, paneId) => terminalHandles.get(paneId)?.find("next")),
  "terminal.find_prev": onPane((_, paneId) => terminalHandles.get(paneId)?.find("prev")),
  "terminal.copy_last_output": onPane((_, paneId) => void copyLastOutput(paneId, toast)),
  "billing.plans": () => useUi.getState().openUpgrade(),
  "help.shortcuts": () => useUi.getState().setCheatsheet(!useUi.getState().cheatsheetOpen),
  "help.copy_diagnostics": () => void copyDiagnostics(toast),
  "help.open_logs": () => void invoke("app_open_logs_folder").catch(() => undefined),
};

/** How often each command's handler ran (debug builds read it: the E2E
 * check that the palette and shortcuts share one handler). */
export const commandRuns = new Map<string, number>();

/** Runs a command if it's enabled. Returns whether it ran. */
export function runCommand(id: string): boolean {
  const spec = COMMANDS.find((c) => c.id === id);
  const handler = handlers[id];
  if (!spec || !handler || !isEnabled(spec)) return false;
  commandRuns.set(id, (commandRuns.get(id) ?? 0) + 1);
  handler();
  return true;
}
