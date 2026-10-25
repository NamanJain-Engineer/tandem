// The sidebar's Ports tab (F-EX-03, UI-142): TCP ports listening in panes'
// process trees, refreshed every 2 s while the tab shows. Open loads the port
// in a sidebar browser tab; Stop asks first, and the core checks the process
// is still the pane's before it signals.
import { useCallback, useEffect, useState } from "react";
import { Plug, RefreshCw } from "lucide-react";
import type { IpcError, ListeningPort, StopOutcome } from "@tandem/protocol";
import {
  Badge,
  Banner,
  Button,
  ConfirmDialog,
  EmptyState,
  IconButton,
  Skeleton,
  Tooltip,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { copyText } from "../../ipc/clipboard";
import { invoke } from "../../ipc/invoke";
import { useLayout } from "../../stores/layout";
import { useSidebar } from "../../stores/sidebar";
import { useWorkspaces } from "../../stores/workspaces";
import { switchMode } from "../shell/actions";
import { PaneUsageSection } from "./PaneUsage";

/** Ports refresh this often while the tab is on screen. */
export const REFRESH_MS = 2_000;

type Load = { at: "loading" } | { at: "error"; message: string } | { at: "ready"; ports: ListeningPort[] };
type Ask = { port: ListeningPort; force: boolean } | null;

/** F-WT-06: the core says where to open it (`<slug>.localhost` for a
 * worktree's server where that resolves). */
export function urlOf(p: ListeningPort): string {
  return p.url || `http://localhost:${p.port}`;
}

/** F-WT-06: ports in order, a worktree's after its header. Ports outside
 * worktrees come first, ungrouped. */
export function groupByWorktree<T extends { laneBranch: string | null }>(
  ports: T[],
): Array<{ branch: string | null; ports: T[] }> {
  const groups = new Map<string, T[]>();
  for (const p of ports) {
    const key = p.laneBranch ?? "";
    groups.set(key, [...(groups.get(key) ?? []), p]);
  }
  const plain = groups.get("") ?? [];
  const lanes = [...groups.entries()].filter(([k]) => k !== "").sort(([a], [b]) => a.localeCompare(b));
  return [
    ...(plain.length ? [{ branch: null, ports: plain }] : []),
    ...lanes.map(([branch, list]) => ({ branch, ports: list })),
  ];
}

/** Opens `url` in a new sidebar browser tab. */
function openInBrowser(url: string): void {
  const sidebar = useSidebar.getState();
  const id = sidebar.openTab("browser");
  sidebar.setPage(id, null, url);
}

/** Shows the pane that started the port. */
export function focusPane(paneId: string, workspaceId: string | null): void {
  const layout = useLayout.getState();
  for (const [tabId, tab] of Object.entries(layout.tabs)) {
    if (tab.panes[paneId]) {
      const ws = useWorkspaces.getState();
      if (workspaceId) ws.select(workspaceId);
      if (workspaceId) ws.selectTab(workspaceId, tabId);
      layout.focus(tabId, paneId);
      switchMode("code");
      return;
    }
  }
}

export function PortsPanel({ visible = true }: { visible?: boolean }) {
  const [state, setState] = useState<Load>({ at: "loading" });
  const [ask, setAsk] = useState<Ask>(null);
  const [stuck, setStuck] = useState<Record<string, true>>({});
  const { show } = useToast();

  const load = useCallback(async () => {
    try {
      setState({ at: "ready", ports: await invoke<ListeningPort[]>("ports_list") });
    } catch (e) {
      setState({ at: "error", message: (e as IpcError).message ?? String(e) });
    }
  }, []);

  // Never scans while hidden.
  useEffect(() => {
    if (!visible) return;
    void load();
    const timer = setInterval(() => void load(), REFRESH_MS);
    return () => clearInterval(timer);
  }, [visible, load]);

  const stop = async (target: NonNullable<Ask>) => {
    const p = target.port;
    try {
      const out = await invoke<StopOutcome>(
        "ports_stop",
        { paneId: p.paneId, pid: p.pid, startTime: p.startTime, force: target.force },
        { timeoutMs: 15_000 },
      );
      const key = `${p.pid}:${p.port}`;
      if (out.stopped) {
        setStuck(({ [key]: _, ...rest }) => rest);
        show({ tone: "success", title: t("ports.stopped", { process: p.process, port: p.port }) });
      } else {
        setStuck((s) => ({ ...s, [key]: true }));
      }
      await load();
    } catch (e) {
      show({ tone: "error", title: (e as IpcError).message ?? String(e) });
      await load();
    }
  };

  const header = (
    <div className="flex h-9 shrink-0 items-center gap-2 border-b border-divider px-2">
      <span className="min-w-0 flex-1 truncate text-sm font-medium text-fg">{t("ports.title")}</span>
      <IconButton
        size="sm"
        data-testid="ports-refresh"
        icon={<RefreshCw size={13} />}
        label={t("ports.refresh")}
        onClick={() => void load()}
      />
    </div>
  );

  let body;
  if (state.at === "loading") {
    body = (
      <div className="space-y-2 p-3" data-testid="ports-loading">
        <Skeleton className="h-10" />
        <Skeleton className="h-10" />
      </div>
    );
  } else if (state.at === "error") {
    body = (
      <div className="p-3">
        <Banner tone="error" action={{ label: t("ports.retry"), onClick: () => void load() }}>
          {t("ports.error")}
        </Banner>
      </div>
    );
  } else if (state.ports.length === 0) {
    body = <EmptyState icon={<Plug size={18} />} title={t("ports.empty")} body={t("ports.empty.body")} />;
  } else {
    body = (
      <ul className="min-h-0 flex-1 overflow-auto p-1" aria-label={t("ports.list")}>
        {groupByWorktree(state.ports).flatMap((group) => [
          ...(group.branch
            ? [
                <li
                  key={`group:${group.branch}`}
                  data-testid="port-group"
                  data-branch={group.branch}
                  className="px-2 pb-0.5 pt-2 text-xs font-medium text-fg-subtle"
                >
                  {group.ports[0]?.lanePorts
                    ? t("ports.worktreeBlock", {
                        branch: group.branch,
                        start: String(group.ports[0].lanePorts.start),
                        end: String(group.ports[0].lanePorts.end),
                      })
                    : t("ports.worktree", { branch: group.branch })}
                </li>,
              ]
            : []),
          ...group.ports.map((p) => {
            const key = `${p.pid}:${p.port}`;
            return (
              <li
                key={key}
                data-testid="port-row"
                data-port={p.port}
                data-pid={p.pid}
                data-pane={p.paneId}
                data-branch={p.laneBranch ?? ""}
                className="flex flex-col gap-1 rounded-sm px-2 py-1.5 hover:bg-surface-2"
              >
                <div className="flex min-w-0 items-center gap-2">
                  <span className="font-mono text-lg tabular-nums text-fg">
                    {t("ports.port", { port: p.port })}
                  </span>
                  <span className="min-w-0 truncate text-sm text-fg-muted">
                    {t("ports.process", { process: p.process, pid: p.pid })}
                  </span>
                  <button
                    type="button"
                    className="min-w-0 truncate text-sm text-fg underline-offset-2 hover:underline focus-visible:shadow-focus focus-visible:outline-none"
                    onClick={() => focusPane(p.paneId, p.workspaceId)}
                  >
                    {[p.paneName ?? t("ports.pane"), p.workspaceName].filter(Boolean).join(" · ")}
                  </button>
                  <span className="ml-auto shrink-0">
                    {p.allInterfaces ? (
                      <Tooltip label={t("ports.network.tip")}>
                        <span tabIndex={0} data-testid="port-network">
                          <Badge variant="waiting">{t("ports.network")}</Badge>
                        </span>
                      </Tooltip>
                    ) : (
                      <Badge variant="outline">{t("ports.local")}</Badge>
                    )}
                  </span>
                </div>
                <div className="flex items-center gap-1.5">
                  <Button
                    size="sm"
                    variant="secondary"
                    data-testid="port-open"
                    onClick={() => openInBrowser(urlOf(p))}
                  >
                    {t("ports.open")}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() =>
                      void copyText(urlOf(p)).then((ok) =>
                        show(
                          ok
                            ? { tone: "success", title: t("ports.copied") }
                            : { tone: "error", title: t("ports.copyFailed") },
                        ),
                      )
                    }
                  >
                    {t("ports.copy")}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    data-testid="port-stop"
                    onClick={() => setAsk({ port: p, force: false })}
                  >
                    {t("ports.stop")}
                  </Button>
                  {stuck[key] && (
                    <span
                      className="ml-1 flex items-center gap-1.5 text-sm text-waiting-text"
                      data-testid="port-stuck"
                    >
                      {t("ports.stillRunning")}
                      <Button size="sm" variant="danger" onClick={() => setAsk({ port: p, force: true })}>
                        {t("ports.force")}
                      </Button>
                    </span>
                  )}
                </div>
              </li>
            );
          }),
        ])}
      </ul>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="ports-panel">
      {header}
      <div className="flex min-h-0 flex-1 flex-col overflow-auto">
        <PaneUsageSection visible={visible} />
        {body}
      </div>
      <ConfirmDialog
        open={ask !== null}
        onOpenChange={(o) => !o && setAsk(null)}
        danger
        title={
          ask
            ? ask.force
              ? t("ports.force.title", { process: ask.port.process, port: ask.port.port })
              : t("ports.stop.title", { process: ask.port.process, port: ask.port.port })
            : ""
        }
        body={
          ask
            ? ask.force
              ? t("ports.force.body", { pid: ask.port.pid })
              : t("ports.stop.body", { pid: ask.port.pid })
            : ""
        }
        confirmLabel={ask?.force ? t("ports.force.confirm") : t("ports.stop.confirm")}
        onConfirm={() => {
          const target = ask;
          setAsk(null);
          if (target) void stop(target);
        }}
      />
    </div>
  );
}
