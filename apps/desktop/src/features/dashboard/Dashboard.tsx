import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import { ChevronDown, ChevronRight, LayoutDashboard, MoreHorizontal, Search, SquareTerminal, X } from "lucide-react";
import type { AgentStatus, DashboardItem } from "@tandem/protocol";
import {
  Button,
  ConfirmDialog,
  EmptyState,
  EngineGlyph,
  IconButton,
  Input,
  Menu,
  SegmentedControl,
  Skeleton,
  StatusDot,
  cx,
  useDelayedFlag,
  useToast,
  type MenuEntry,
  type Status,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useDashboard, type Filter } from "../../stores/dashboard";
import { useEngines } from "../../stores/engines";
import { useSetting } from "../../stores/settings";
import { useSidebar } from "../../stores/sidebar";
import { useWorkspaces } from "../../stores/workspaces";
import { answer, closeAgent, closeIdle, jumpTo, stopAgent } from "./actions";
import { cardMeta, cardTitle, closeIdleTargets, counts, group, visible, type Group, type Lanes } from "./model";
import { isWorktree } from "../../stores/worktrees";

const view = (s: AgentStatus): Status => (s === "failed" ? "error" : s);

const BAR: Record<AgentStatus, string> = {
  waiting: "bg-waiting",
  working: "bg-working",
  done: "bg-done",
  idle: "bg-idle",
  failed: "bg-error",
};

const SUB: Record<AgentStatus, (n: number) => string> = {
  waiting: (n) => t("dashboard.sub.waiting", { n }),
  working: (n) => t("dashboard.sub.working", { n }),
  done: (n) => t("dashboard.sub.done", { n }),
  idle: (n) => t("dashboard.sub.idle", { n }),
  failed: (n) => t("dashboard.sub.failed", { n }),
};

const CHIP: Record<Filter, { label: (n: number) => string; name: () => string }> = {
  all: { label: (n) => t("dashboard.chip.all", { n }), name: () => t("dashboard.chipName.all") },
  waiting: { label: (n) => t("dashboard.chip.waiting", { n }), name: () => t("dashboard.chipName.waiting") },
  working: { label: (n) => t("dashboard.chip.working", { n }), name: () => t("dashboard.chipName.working") },
  done: { label: (n) => t("dashboard.chip.done", { n }), name: () => t("dashboard.chipName.done") },
  idle: { label: (n) => t("dashboard.chip.idle", { n }), name: () => t("dashboard.chipName.idle") },
  failed: { label: (n) => t("dashboard.chip.failed", { n }), name: () => t("dashboard.chipName.failed") },
};

const STATUS_LABEL: Record<AgentStatus, () => string> = {
  waiting: () => t("agentStatus.waiting"),
  working: () => t("agentStatus.working"),
  done: () => t("agentStatus.done"),
  idle: () => t("agentStatus.idle"),
  failed: () => t("agentStatus.error"),
};

const GROUP_LABEL: Record<AgentStatus, (n: number) => string> = {
  waiting: (n) => t("dashboard.group.waiting", { n }),
  working: (n) => t("dashboard.group.working", { n }),
  done: (n) => t("dashboard.group.done", { n }),
  idle: (n) => t("dashboard.group.idle", { n }),
  failed: (n) => t("dashboard.group.failed", { n }),
};

/** "4m" / "1h 12m". */
function elapsed(since: number, now: number): string {
  const m = Math.max(0, Math.floor((now - since) / 60_000));
  return m < 60 ? t("time.minutes", { n: m }) : t("time.hoursMinutes", { h: Math.floor(m / 60), m: m % 60 });
}

function useNow(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(id);
  }, []);
  return now;
}

function AgentCard({ item, title, now }: { item: DashboardItem; title: string; now: number }) {
  const showPreview = useSetting("show_status_previews", true);
  const logo = useEngines((s) => s.list.find((e) => e.spec.id === item.engineId)?.spec.logo) ?? item.engineLogo;
  const [confirm, setConfirm] = useState(false);
  const { show } = useToast();
  const busy = item.status === "working" || item.status === "waiting";
  const approval = item.pendingApproval;
  const close = () => (busy ? setConfirm(true) : void closeAgent(item).then(() => show({ title: t("dashboard.closed", { title }) })));
  const decide = (allow: boolean) =>
    approval &&
    void answer(approval.id, allow).then((err) => err && show({ tone: "error", title: err.message }));
  const menu: MenuEntry[] = [
    { id: "open", label: t("dashboard.open"), onSelect: () => void jumpTo(item) },
    ...(item.status === "done"
      ? [{ id: "seen", label: t("dashboard.markSeen"), onSelect: () => void invoke("status_mark_seen", { key: item.key }).catch(() => undefined) }]
      : []),
    ...(item.status === "working"
      ? [{ id: "stop", label: t("dashboard.stop"), danger: true, onSelect: () => void stopAgent(item) }]
      : []),
    { id: "sep", type: "separator" as const },
    { id: "close", label: t("dashboard.close"), danger: true, onSelect: close },
  ];
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Enter" && e.target === e.currentTarget) {
      e.preventDefault();
      void jumpTo(item);
    }
  };
  return (
    <li
      data-testid="agent-card"
      data-key={item.key}
      data-status={item.status}
      tabIndex={0}
      aria-label={`${title}, ${STATUS_LABEL[item.status]()}`}
      onClick={() => void jumpTo(item)}
      onKeyDown={onKey}
      className={cx(
        "group flex cursor-pointer flex-col gap-1 rounded-md border bg-surface-2 px-3 py-2 outline-none hover:bg-surface-3 focus-visible:shadow-focus",
        item.status === "waiting" ? "border-waiting" : "border-border",
      )}
    >
      <div className="flex items-center gap-2">
        <StatusDot status={view(item.status)} size={8} />
        {logo ? <EngineGlyph letters={logo} size={16} /> : <SquareTerminal size={14} aria-hidden="true" className="text-fg-muted" />}
        <span data-testid="agent-card-title" className="min-w-0 flex-1 truncate text-base font-medium text-fg">
          {title}
        </span>
        <span className="shrink-0 text-sm tabular-nums text-fg-muted">
          {STATUS_LABEL[item.status]()} · {elapsed(item.since, now)}
        </span>
        <span className="flex shrink-0 items-center opacity-0 group-focus-within:opacity-100 group-hover:opacity-100" onClick={(e) => e.stopPropagation()}>
          <Menu align="end" trigger={<IconButton size="sm" icon={<MoreHorizontal size={14} />} label={t("dashboard.menu", { title })} />} entries={menu} />
          <IconButton size="sm" icon={<X size={14} />} label={t("dashboard.closeCard", { title })} onClick={close} />
        </span>
      </div>
      {cardMeta(item) && <p className="truncate text-sm text-fg-muted">{cardMeta(item)}</p>}
      {showPreview && (approval?.summary ?? item.preview) && (
        <p data-testid="agent-card-preview" className="line-clamp-2 text-sm text-fg-subtle">
          {approval ? t("dashboard.wants", { summary: approval.summary }) : item.preview}
        </p>
      )}
      {approval && (
        <div className="flex gap-2 pt-1" onClick={(e) => e.stopPropagation()}>
          <Button size="sm" variant="primary" data-testid="card-allow" onClick={() => decide(true)}>
            {t("dashboard.allowOnce")}
          </Button>
          <Button size="sm" variant="secondary" data-testid="card-deny" onClick={() => decide(false)}>
            {t("dashboard.deny")}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => void jumpTo(item)}>
            {t("dashboard.open")}
          </Button>
        </div>
      )}
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        title={t("pane.stopConfirm", { name: item.humanName ?? title })}
        body={t("pane.stopBody")}
        confirmLabel={t("pane.stopAndClose")}
        cancelLabel={t("pane.keepRunning")}
        danger
        onConfirm={() => void closeAgent(item)}
      />
    </li>
  );
}

function FilterChip({ id, label, active }: { id: Filter; label: string; active: boolean }) {
  return (
    <button
      type="button"
      aria-pressed={active}
      data-testid={`filter-${id}`}
      onClick={() => useDashboard.getState().setFilter(id)}
      className={cx(
        "h-6 shrink-0 rounded-full border px-2.5 text-sm focus-visible:shadow-focus focus-visible:outline-none",
        active ? "border-accent bg-accent-soft text-fg" : "border-border text-fg-muted hover:bg-surface-3",
      )}
    >
      {label}
    </button>
  );
}

/** UI-42 / UI-43, F-DB-01: every running agent across modes. */
export function Dashboard() {
  const { items, loaded, query, filter, groupBy, collapsed } = useDashboard();
  const engines = useEngines((s) => s.list);
  const workspaces = useWorkspaces((s) => s.list);
  const order = useMemo(() => workspaces.map((w) => w.id), [workspaces]);
  const fullscreen = useSidebar((s) => s.fullscreen);
  const now = useNow();
  const { show } = useToast();
  const [confirmIdle, setConfirmIdle] = useState<string[] | null>(null);
  const slow = useDelayedFlag(!loaded, 150);

  useEffect(() => {
    if (!loaded) void useDashboard.getState().load();
  }, [loaded]);

  const all = useMemo(() => Object.values(items), [items]);
  const titles = useMemo(() => {
    const patterns = (id: string | null) => engines.find((e) => e.spec.id === id)?.spec.titlePatterns ?? [];
    return Object.fromEntries(all.map((i) => [i.key, cardTitle(i, patterns(i.engineId))]));
  }, [all, engines]);
  const c = counts(all);
  const shown = visible(all, filter, query, (i) => titles[i.key] ?? "");
  const lanes = useMemo<Lanes>(
    () =>
      Object.fromEntries(
        workspaces.filter(isWorktree).map((w) => [
          w.id,
          {
            parentId: w.parentId ?? "",
            parentName: workspaces.find((p) => p.id === w.parentId)?.name ?? "",
            branch: w.laneBranch ?? w.name,
          },
        ]),
      ),
    [workspaces],
  );
  const groups = group(shown, groupBy, order, t("dashboard.noFolder"), lanes);
  const idle = closeIdleTargets(shown);
  const narrowed = filter !== "all" || query.trim() !== "";

  if (!loaded)
    return slow ? (
      <div className="flex flex-col gap-2 p-3" data-testid="dashboard-loading">
        {[0, 1, 2].map((n) => (
          <Skeleton key={n} height={64} className="rounded-md" />
        ))}
      </div>
    ) : null;

  if (all.length === 0)
    return (
      <div data-testid="dashboard" className="p-3">
        <EmptyState icon={<LayoutDashboard size={20} />} title={t("dashboard.emptyTitle")} body={t("dashboard.emptyBody")} />
      </div>
    );

  const segments: { status: AgentStatus; n: number }[] = (["waiting", "working", "done", "idle", "failed"] as const)
    .map((status) => ({ status, n: c[status] }))
    .filter((x) => x.status !== "failed" || x.n > 0);

  const renderGroup = (g: Group) => {
    const open = !collapsed[g.id];
    const total = g.items.length + (g.children ?? []).reduce((n, c) => n + c.items.length, 0);
    const label = g.status ? GROUP_LABEL[g.status](g.items.length) : `${g.label} · ${total}`;
    return (
      <section key={g.id} aria-label={label} data-testid={`group-${g.id}`} className="flex flex-col gap-1.5">
        <div className="flex h-6 items-center gap-1">
          <button
            type="button"
            aria-expanded={open}
            onClick={() => useDashboard.getState().toggleGroup(g.id)}
            className="flex items-center gap-1 rounded-xs text-2xs font-semibold uppercase tracking-caps text-fg-subtle focus-visible:shadow-focus focus-visible:outline-none"
          >
            {open ? <ChevronDown size={12} aria-hidden="true" /> : <ChevronRight size={12} aria-hidden="true" />}
            {label}
          </button>
          <span className="flex-1" />
          {g.status === "idle" && idle.length > 0 && (
            <Button size="sm" variant="secondary" data-testid="close-idle" onClick={() => setConfirmIdle(idle)}>
              {narrowed ? t("dashboard.closeMatchingIdle", { n: idle.length }) : t("dashboard.closeIdle", { n: idle.length })}
            </Button>
          )}
        </div>
        {open && (
          <ul className={cx("grid gap-2", fullscreen ? "grid-cols-[repeat(auto-fill,minmax(280px,1fr))] 2xl:grid-cols-4" : "grid-cols-1")}>
            {g.items.map((i) => (
              <AgentCard key={i.key} item={i} title={titles[i.key] ?? ""} now={now} />
            ))}
          </ul>
        )}
        {open && g.children && (
          <div data-testid={`worktrees-${g.id}`} className="ml-1.5 flex flex-col gap-2 border-l border-divider pl-2.5">
            {g.children.map(renderGroup)}
          </div>
        )}
      </section>
    );
  };

  return (
    <div data-testid="dashboard" className="flex flex-col gap-3 p-3">
      <header className="flex flex-col gap-1.5">
        <h2 data-testid="dashboard-count" className="text-xl font-semibold text-fg">
          {t("dashboard.count", { n: c.total })}
        </h2>
        <p data-testid="dashboard-subtitle" className="flex flex-wrap gap-x-1 text-sm text-fg-muted">
          {segments.map((x, i) => (
            <span key={x.status}>
              {i > 0 && " · "}
              <button
                type="button"
                className="rounded-xs hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
                onClick={() => useDashboard.getState().setFilter(x.status)}
              >
                {SUB[x.status](x.n)}
              </button>
            </span>
          ))}
        </p>
        <div role="img" aria-label={t("dashboard.barLabel")} className="flex h-1.5 overflow-hidden rounded-full bg-surface-3">
          {segments.map((x) =>
            x.n > 0 ? (
              <span
                key={x.status}
                aria-label={SUB[x.status](x.n)}
                className={cx("h-full", BAR[x.status])}
                style={{ width: `${(x.n / c.total) * 100}%` }}
              />
            ) : null,
          )}
        </div>
      </header>
      <div className="flex items-center gap-2">
        <div className="relative min-w-0 flex-1">
          <Search size={13} aria-hidden="true" className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-fg-subtle" />
          <Input
            aria-label={t("dashboard.search")}
            placeholder={t("dashboard.search")}
            data-testid="dashboard-search"
            value={query}
            className="pl-7"
            onChange={(e) => useDashboard.getState().setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key !== "Escape") return;
              e.stopPropagation();
              if (query) useDashboard.getState().setQuery("");
              else e.currentTarget.blur();
            }}
          />
        </div>
        <SegmentedControl
          size="sm"
          label={t("dashboard.groupBy")}
          value={groupBy}
          onValueChange={(v) => useDashboard.getState().setGroupBy(v === "project" ? "project" : "status")}
          items={[
            { id: "status", label: t("dashboard.byStatus") },
            { id: "project", label: t("dashboard.byProject") },
          ]}
        />
      </div>
      <div role="group" aria-label={t("dashboard.filters")} className="flex flex-wrap gap-1.5">
        {(["all", "waiting", "working", "done", "idle", "failed"] as const)
          .filter((f) => f !== "failed" || c.failed > 0)
          .map((f) => {
            const total = f === "all" ? c.total : c[f];
            const matching = query.trim() ? visible(all, f, query, (i) => titles[i.key] ?? "").length : total;
            const label = CHIP[f].label(total);
            return (
              <FilterChip
                key={f}
                id={f}
                active={filter === f}
                label={matching === total ? label : t("dashboard.chipOf", { label: CHIP[f].name(), n: matching, total })}
              />
            );
          })}
      </div>
      {groups.length === 0 ? (
        <EmptyState
          title={t("dashboard.noMatch", { q: query })}
          primary={
            <Button size="sm" variant="secondary" onClick={() => useDashboard.setState({ query: "", filter: "all" })}>
              {t("dashboard.clearSearch")}
            </Button>
          }
        />
      ) : (
        groups.map(renderGroup)
      )}
      <ConfirmDialog
        open={confirmIdle !== null}
        onOpenChange={(o) => !o && setConfirmIdle(null)}
        title={t("dashboard.closeIdleTitle", { n: confirmIdle?.length ?? 0 })}
        body={t("dashboard.closeIdleBody")}
        confirmLabel={t("dashboard.closeIdleConfirm", { n: confirmIdle?.length ?? 0 })}
        onConfirm={() => {
          const keys = confirmIdle ?? [];
          setConfirmIdle(null);
          void closeIdle(keys).then((closed) => show({ title: t("dashboard.closedIdle", { n: closed.length }) }));
        }}
      />
    </div>
  );
}
