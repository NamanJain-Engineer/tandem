import { useEffect, useMemo, useState, type ReactNode } from "react";
import { FolderPlus, LayoutGrid, Plus, X } from "lucide-react";
import type { Tab } from "@tandem/protocol";
import {
  Button,
  EmptyState,
  IconButton,
  cx,
  terminalThemeFor,
  useTheme,
  useToast,
  type TerminalThemeSetting,
} from "@tandem/ui";
import { t } from "../../i18n";
import { useLayout } from "../../stores/layout";
import { useSetting } from "../../stores/settings";
import { useWorkspaces } from "../../stores/workspaces";
import { pickAndAddWorkspace } from "../shell/actions";
import { PaneDndProvider, useNewTabTarget, useDrag } from "./dnd";
import { PaneGrid } from "./PaneGrid";
import { PanePicker } from "./PanePicker";
import type { TerminalOptions } from "./TerminalView";

function useTerminalOptions(): TerminalOptions {
  const { resolved, highContrast } = useTheme();
  const themeSetting = useSetting<TerminalThemeSetting>("terminal_theme", "follow_app");
  const fontFamily = useSetting("term_font_family", "JetBrains Mono");
  const fontSize = useSetting("term_font_size", 13);
  const cursorStyle = useSetting<"bar" | "block" | "underline">("term_cursor_style", "bar");
  const cursorBlink = useSetting("term_cursor_blink", false);
  const scrollback = useSetting("term_scrollback", 10_000);
  return useMemo(
    () => ({
      theme: terminalThemeFor(themeSetting, resolved, highContrast),
      fontFamily,
      fontSize,
      cursorStyle,
      cursorBlink,
      scrollback,
    }),
    [themeSetting, resolved, highContrast, fontFamily, fontSize, cursorStyle, cursorBlink, scrollback],
  );
}

/** The tab strip; while a pane is dragged it's the "New tab" target. */
function TabStrip({
  children,
  workspaceId,
  label,
}: {
  children: ReactNode;
  workspaceId: string;
  label: string;
}) {
  const { setNodeRef, isOver } = useNewTabTarget(workspaceId);
  const dragging = useDrag((s) => s.active !== null);
  return (
    <div
      ref={setNodeRef}
      className={cx(
        "flex min-w-0 flex-1 items-center gap-1 rounded-sm",
        dragging && "outline-dashed outline-1 outline-border-strong",
        isOver && "bg-accent-soft outline-accent",
      )}
    >
      <ul aria-label={label} className="flex min-w-0 items-center gap-1 overflow-x-auto">
        {children}
      </ul>
    </div>
  );
}

export function CodeMain({ visible }: { visible: boolean }) {
  const { list, activeId, activeTab, selectTab, createTab, closeTab } = useWorkspaces();
  const { show } = useToast();
  const options = useTerminalOptions();
  const [visited, setVisited] = useState<Record<string, Tab>>({});
  const active = list.find((w) => w.id === activeId) ?? null;
  const activeTabId = active ? activeTab[active.id] : undefined;

  useEffect(() => {
    const tab = active?.tabs.find((x) => x.id === activeTabId);
    if (tab && !visited[tab.id]) setVisited((v) => ({ ...v, [tab.id]: tab }));
  }, [active, activeTabId, visited]);

  // Drop grids whose tab is gone.
  useEffect(() => {
    const alive = new Set(list.flatMap((w) => w.tabs.map((x) => x.id)));
    const gone = Object.keys(visited).filter((id) => !alive.has(id));
    if (gone.length) {
      for (const id of gone) useLayout.getState().forgetTab(id);
      setVisited((v) => Object.fromEntries(Object.entries(v).filter(([id]) => alive.has(id))));
    }
  }, [list, visited]);

  if (!active) {
    return (
      <div className="flex h-full items-center justify-center">
        <EmptyState
          icon={<FolderPlus size={20} />}
          title={t("code.empty.title")}
          body={t("code.empty.body")}
          primary={
            <Button variant="primary" onClick={() => void pickAndAddWorkspace(show)}>
              {t("code.empty.add")}
            </Button>
          }
        />
      </div>
    );
  }

  return (
    <PaneDndProvider>
      <div className="flex h-full min-h-0 flex-col gap-1.5">
        <div className="flex h-8 shrink-0 items-center gap-1">
          <TabStrip workspaceId={active.id} label={t("code.tabs", { name: active.name })}>
            {active.tabs.map((tab) => {
              const selected = tab.id === activeTabId;
              return (
                <li
                  key={tab.id}
                  className={cx(
                    "group flex h-7 items-center rounded-sm text-base",
                    selected ? "bg-surface-3 text-fg" : "text-fg-muted hover:bg-surface-2",
                  )}
                >
                  <button
                    type="button"
                    aria-current={selected ? "page" : undefined}
                    aria-controls={`panel-${tab.id}`}
                    onClick={() => selectTab(active.id, tab.id)}
                    className="h-full whitespace-nowrap rounded-sm px-2.5 focus-visible:shadow-focus focus-visible:outline-none"
                  >
                    {tab.name}
                  </button>
                  <button
                    type="button"
                    aria-label={t("code.closeTab", { name: tab.name })}
                    onClick={() => void closeTab(active.id, tab.id)}
                    className="mr-1 inline-flex size-5 items-center justify-center rounded-xs text-fg-subtle opacity-0 hover:bg-surface-4 hover:text-fg focus-visible:opacity-100 focus-visible:shadow-focus focus-visible:outline-none group-hover:opacity-100"
                  >
                    <X size={12} aria-hidden="true" />
                  </button>
                </li>
              );
            })}
          </TabStrip>
          <IconButton
            size="sm"
            icon={<Plus size={14} />}
            label={t("code.newTab")}
            kbd={["Mod", "T"]}
            onClick={() => void createTab(active.id)}
          />
          {activeTabId && (
            <Button
              size="sm"
              variant="ghost"
              iconLeft={<LayoutGrid size={14} />}
              onClick={() => useLayout.getState().arrange(activeTabId)}
            >
              {t("code.arrange")}
            </Button>
          )}
        </div>
        <div className="relative min-h-0 flex-1">
          {active.missing && (
            <div className="absolute inset-0 z-10 flex items-center justify-center bg-bg">
              <EmptyState title={t("code.missing.title")} body={t("code.missing.body")} />
            </div>
          )}
          {Object.values(visited).map((tab) => {
            const shown = visible && tab.id === activeTabId && tab.workspaceId === active.id;
            return (
              <div
                key={tab.id}
                id={`panel-${tab.id}`}
                className="absolute inset-0"
                style={{ display: shown ? "block" : "none" }}
              >
                <PaneGrid
                  workspace={list.find((w) => w.id === tab.workspaceId) ?? active}
                  tabId={tab.id}
                  visible={shown}
                  options={options}
                />
              </div>
            );
          })}
        </div>
        <PanePicker workspaceName={active.name} />
      </div>
    </PaneDndProvider>
  );
}
