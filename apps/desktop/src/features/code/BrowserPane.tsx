// A browser pane (F-CD-17): the browser in a Code tab, tiled with the
// terminals. It runs no process. "Open in sidebar" moves its page, history
// and all, into a sidebar tab; the sidebar's "Move to a pane" brings it back.
import { memo } from "react";
import { useDraggable } from "@dnd-kit/core";
import { Globe } from "lucide-react";
import type { PaneRecord, SidebarTab } from "@tandem/protocol";
import { cx, useToast, type MenuEntry } from "@tandem/ui";
import { t } from "../../i18n";
import { pageTitle, useBrowser } from "../../stores/browser";
import { useCloseConfirm } from "../../stores/closeConfirm";
import { useFlash } from "../../stores/focus";
import { useLayout } from "../../stores/layout";
import { useSidebar } from "../../stores/sidebar";
import { useWorkspaces } from "../../stores/workspaces";
import { BrowserView } from "../browser/BrowserView";
import { DropZones, moveToNewTab, useDrag, type PaneDrag } from "./dnd";
import { useDropTarget } from "./drops";
import { PaneChrome } from "./PaneChrome";
import { useShortcutKeys } from "../commands/useShortcutKeys";

/** "Open in sidebar": a sidebar tab takes the page, then the pane goes. */
export async function openInSidebar(tabId: string, pane: PaneRecord): Promise<string | null> {
  const sidebar = useSidebar.getState();
  const page = useBrowser.getState().pages[pane.id];
  const id = sidebar.openTab("browser");
  sidebar.setPage(id, page ? pageTitle(page) : null, page?.url || pane.browserUrl || null);
  // A pane that never opened its page has nothing to move: the tab opens
  // its address instead.
  const err = page ? await useBrowser.getState().move(pane.id, id) : null;
  if (err && err.code !== "not_found") {
    sidebar.close(id);
    return err.message;
  }
  await useLayout.getState().close(tabId, pane.id);
  return null;
}

/** "Move to a pane": the page goes into the active Code tab, beside its
 * focused pane, and the sidebar tab goes. */
export async function moveToPane(tab: SidebarTab): Promise<string | null> {
  const ws = useWorkspaces.getState();
  const codeTab = ws.activeId ? ws.activeTab[ws.activeId] : undefined;
  if (!codeTab) return null;
  const layout = useLayout.getState();
  const page = useBrowser.getState().pages[tab.id];
  // With no page yet, the pane opens the tab's address itself.
  const res = await layout.split(codeTab, layout.tabs[codeTab]?.active ?? null, "right", { kind: "browser", url: page ? null : tab.url });
  if (!res.ok) return res.error.message;
  if (page) {
    const err = await useBrowser.getState().move(tab.id, res.pane.id);
    if (err) return err.message;
  }
  useSidebar.getState().close(tab.id);
  return null;
}

type Props = {
  tabId: string;
  pane: PaneRecord;
  title: string;
  visible: boolean;
  focused: boolean;
  solo: boolean;
  maximized: boolean;
  /** Card view's card or a collapsed session covers the pane (F-CD-11). */
  cover: "card" | "line" | null;
};

export const BrowserPane = memo(function BrowserPane({ tabId, pane, title, visible, focused, solo, maximized, cover }: Props) {
  const keys = useShortcutKeys();
  const { show } = useToast();
  const layout = useLayout.getState;
  const dragging = useDrag((st) => st.active !== null);
  const dropping = useDropTarget((st) => st.paneId === pane.id);
  const flashing = useFlash((st) => st.flashing[pane.id] !== undefined);
  const drag: PaneDrag = { kind: "pane", paneId: pane.id, tabId, title };
  const { attributes, listeners, setNodeRef } = useDraggable({ id: `pane:${pane.id}`, data: drag });
  const split = (side: "right" | "bottom") => {
    if (layout().requestSplit(tabId, pane.id, side) === "no_room") show({ tone: "warning", title: t("split.noRoom") });
  };
  const toSidebar = () =>
    void openInSidebar(tabId, pane).then((message) => message && show({ tone: "error", title: message }));
  const menu: MenuEntry[] = [
    { id: "title", type: "label", label: title },
    { id: "split-right", label: t("pane.splitRight"), kbd: keys("pane.split_right"), onSelect: () => split("right") },
    { id: "split-down", label: t("pane.splitDown"), kbd: keys("pane.split_down"), onSelect: () => split("bottom") },
    { id: "new-tab", label: t("pane.moveToNewTab"), onSelect: () => void moveToNewTab(tabId, pane.id) },
    { id: "sidebar", label: t("browser.openInSidebar"), onSelect: toSidebar },
    {
      id: "close",
      label: t("pane.closePane"),
      danger: true,
      kbd: keys("pane.close"),
      onSelect: () => useCloseConfirm.getState().request(tabId, pane.id),
    },
  ];

  return (
    <section
      aria-label={t("pane.label", { name: title })}
      data-testid="pane"
      data-pane-id={pane.id}
      data-kind="browser"
      data-focused={focused ? "true" : undefined}
      data-drop-target={dropping ? "true" : undefined}
      data-flash={flashing ? "true" : undefined}
      className={cx(
        "relative flex h-full min-h-0 flex-col overflow-hidden rounded-md border",
        flashing && "tn-flash",
        dropping && "ring-2 ring-accent",
        focused && !solo ? "border-border-strong" : "border-border",
      )}
      onFocusCapture={() => layout().focus(tabId, pane.id)}
      onPointerDownCapture={() => layout().focus(tabId, pane.id)}
    >
      <PaneChrome
        title={title}
        status="idle"
        focused={focused && !solo}
        solo={solo}
        maximized={maximized}
        menuItems={menu}
        onMaximize={() => layout().toggleMaximize(tabId, pane.id)}
        onClose={() => useCloseConfirm.getState().request(tabId, pane.id)}
        glyph={<Globe size={13} />}
        dragRef={setNodeRef}
        dragProps={{ ...attributes, ...listeners, tabIndex: -1, role: "toolbar" }}
      />
      <div className="relative min-h-0 flex-1">
        {cover && (
          // Like a terminal's card: the page waits behind it (F-CD-11).
          <button
            type="button"
            data-testid={cover === "card" ? "pane-card" : "pane-collapsed"}
            onClick={() => layout().flipCard(tabId, pane.id)}
            className="absolute inset-0 z-10 flex items-center justify-center gap-2 bg-surface-2 text-sm text-fg focus-visible:shadow-focus focus-visible:outline-none"
          >
            <Globe size={14} aria-hidden="true" className="text-fg-muted" />
            <span className="truncate">{title}</span>
          </button>
        )}
        <BrowserView
          id={pane.id}
          url={pane.browserUrl}
          // The page would cover the card, or the drop zones while a pane drags.
          suspended={!visible || cover !== null || dragging}
          actions={[{ id: "sidebar", label: t("browser.openInSidebar"), onSelect: toSidebar }]}
        />
      </div>
      <DropZones paneId={pane.id} tabId={tabId} />
    </section>
  );
});
