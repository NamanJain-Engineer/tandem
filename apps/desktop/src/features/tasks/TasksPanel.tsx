// F-TK-03, UI-80/81: the tracker's list, grouped by status, with quick add,
// archive (row ×) and "Show archived" with Restore. The sidebar's Tasks tab
// and the Tasks page share it; wide enough, the open task sits beside the
// list, else it replaces it with a way back. Creating or assigning a task
// never starts work (F-TK-04): runs start only from Execute.
import { useEffect, useState, type ReactNode } from "react";
import { ListTodo, RotateCcw, X } from "lucide-react";
import type { IssueSummary } from "@tandem/protocol";
import {
  Avatar,
  Banner,
  Button,
  Checkbox,
  EmptyState,
  IconButton,
  Input,
  Skeleton,
  StatusDot,
  cx,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { usePlans } from "../../stores/plan";
import { STATUS_ORDER, useTasks } from "../../stores/tasks";
import { useExecuteBlock } from "./Execute";
import { PlanWithLead, startedStatus } from "./PlanWithLead";
import { TaskDetailView, actorName, priorityLabel, statusLabel } from "./TaskDetail";

/** Wide enough for the list beside the open task. */
export const WIDE_AT = 640;

function useWidth(): [(el: HTMLDivElement | null) => void, number] {
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    if (!el) return;
    const measure = () => setWidth(el.clientWidth);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [el]);
  return [setEl, width];
}

function Row({ issue }: { issue: IssueSummary }) {
  const selectedId = useTasks((s) => s.selectedId);
  const labels = useTasks((s) => s.meta?.labels ?? []);
  const { show: toast } = useToast();
  const archive = async () => {
    const e = await useTasks.getState().archive(issue);
    if (e) toast({ tone: "error", title: e.message });
    else
      toast({
        title: t("tasks.archived", { id: issue.id }),
        action: {
          label: t("tasks.undo"),
          onClick: () => void useTasks.getState().restore({ id: issue.id, revision: issue.revision + 1 }),
        },
      });
  };
  const restore = async () => {
    const e = await useTasks.getState().restore(issue);
    if (e) toast({ tone: "error", title: e.message });
  };
  return (
    <li
      className={cx("group flex items-center", issue.archived && "opacity-60")}
      data-testid="task-row"
      data-id={issue.id}
    >
      <button
        type="button"
        aria-current={issue.id === selectedId ? "true" : undefined}
        onClick={() => void useTasks.getState().select(issue.id)}
        className={cx(
          "flex min-w-0 flex-1 items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm focus-visible:shadow-focus focus-visible:outline-none",
          issue.id === selectedId ? "bg-surface-3 text-fg" : "text-fg-muted hover:bg-surface-2 hover:text-fg",
        )}
      >
        <span className="w-14 shrink-0 font-mono text-xs text-fg-subtle">{issue.id}</span>
        <span className="min-w-0 flex-1 truncate text-fg" data-testid="task-row-title">
          {issue.title}
        </span>
        {issue.labels.slice(0, 2).map((id) => (
          <span
            key={id}
            className="hidden shrink-0 rounded-full border border-border px-1.5 text-2xs text-fg-muted sm:inline"
          >
            {labels.find((l) => l.id === id)?.name ?? id}
          </span>
        ))}
        {issue.priority > 0 && (
          <span className="shrink-0 text-2xs text-fg-subtle" title={priorityLabel(issue.priority)}>
            {t("tasks.priorityShort", { n: issue.priority })}
          </span>
        )}
        {issue.assignee && (
          <span title={actorName(issue.assignee)}>
            <Avatar size={20} id={issue.assignee} name={actorName(issue.assignee)} />
          </span>
        )}
      </button>
      {issue.archived ? (
        <IconButton
          size="sm"
          icon={<RotateCcw size={13} />}
          label={t("tasks.restore", { id: issue.id })}
          onClick={() => void restore()}
          data-testid="task-restore"
        />
      ) : (
        <IconButton
          size="sm"
          icon={<X size={13} />}
          label={t("tasks.archive", { id: issue.id })}
          className="opacity-0 focus-visible:opacity-100 group-hover:opacity-100"
          onClick={() => void archive()}
          data-testid="task-archive"
        />
      )}
    </li>
  );
}

function QuickAdd() {
  const [title, setTitle] = useState("");
  const [error, setError] = useState<string | null>(null);
  const { show: toast } = useToast();
  return (
    <Input
      label={t("tasks.quickAdd")}
      hideLabel
      placeholder={t("tasks.quickAddPlaceholder")}
      value={title}
      error={error ?? undefined}
      onChange={(e) => {
        setTitle(e.target.value);
        setError(null);
      }}
      onKeyDown={async (e) => {
        if (e.key !== "Enter" || !title.trim()) return;
        e.preventDefault();
        const r = await useTasks.getState().quickAdd(title);
        if (r) {
          setError(r.message);
          toast({ tone: "error", title: r.message });
        } else setTitle("");
      }}
      data-testid="task-quick-add"
    />
  );
}

function Groups() {
  const issues = useTasks((s) => s.issues);
  const showArchived = useTasks((s) => s.showArchived);
  const visible = showArchived ? issues : issues.filter((i) => !i.archived);
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-auto p-1.5" data-testid="task-list">
      {STATUS_ORDER.map((status) => {
        const rows = visible.filter((i) => i.status === status);
        if (!rows.length) return null;
        return (
          <section key={status} aria-label={statusLabel(status)} data-testid={`task-group-${status}`}>
            <h3 className="flex items-center gap-2 px-2 pb-1 text-2xs font-medium uppercase tracking-caps text-fg-subtle">
              {statusLabel(status)}
              <span className="tabular-nums">{rows.length}</span>
            </h3>
            <ul className="flex flex-col gap-0.5">
              {rows.map((i) => (
                <Row key={i.id} issue={i} />
              ))}
            </ul>
          </section>
        );
      })}
    </div>
  );
}

function ListPane({ header }: { header?: ReactNode }) {
  const status = useTasks((s) => s.status);
  const error = useTasks((s) => s.error);
  const issues = useTasks((s) => s.issues);
  const showArchived = useTasks((s) => s.showArchived);
  const hasVisible = issues.some((i) => showArchived || !i.archived);
  // "Nothing runs until you choose Execute" only where Execute can run.
  const canRun = useExecuteBlock() === null;
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-testid="tasks-panel">
      {header}
      <div className="flex shrink-0 flex-col gap-2 border-b border-divider p-2">
        <QuickAdd />
        <Checkbox
          label={t("tasks.showArchived")}
          checked={showArchived}
          onCheckedChange={(on) => void useTasks.getState().setShowArchived(on)}
        />
      </div>
      {status === "error" ? (
        <Banner
          tone="error"
          action={{ label: t("tasks.retry"), onClick: () => void useTasks.getState().load() }}
        >
          {error ?? t("tasks.loadError")}
        </Banner>
      ) : status !== "ready" ? (
        <div className="flex flex-col gap-2 p-3" aria-busy="true" data-testid="tasks-loading">
          <Skeleton width="80%" />
          <Skeleton width="60%" />
          <Skeleton width="70%" />
        </div>
      ) : !hasVisible ? (
        <EmptyState
          icon={<ListTodo size={20} />}
          title={t("tasks.empty.title")}
          body={canRun ? t("tasks.empty.body") : t("tasks.empty.bodyTrack")}
        />
      ) : (
        <Groups />
      )}
    </div>
  );
}

/** The tracker; the open task has Execute… (F-TK-04). */
export function TasksPanel({ header }: { header?: ReactNode }) {
  const [ref, width] = useWidth();
  const selectedId = useTasks((s) => s.selectedId);
  useEffect(() => {
    void useTasks.getState().load();
  }, []);
  const wide = width >= WIDE_AT;
  return (
    <div ref={ref} className="flex h-full min-h-0">
      {(wide || !selectedId) && <ListPane header={header} />}
      {selectedId && (
        <div
          className={cx("flex min-h-0 min-w-0 flex-col", wide ? "w-[55%] border-l border-divider" : "flex-1")}
        >
          <TaskDetailView onClose={wide ? undefined : () => void useTasks.getState().select(null)} />
        </div>
      )}
    </div>
  );
}

/** Every plan until it's removed, newest first (F-TK-06, F-TK-07). */
function OpenPlans() {
  const plans = usePlans((s) => s.plans);
  const attempts = useTasks((s) => s.attempts);
  const read = useTasks((s) => s.attemptsStatus === "ready");
  // Every plan until it's removed (F-TK-07), newest first.
  const live = Object.values(plans).sort((a, b) => b.createdAt - a.createdAt);
  // A started plan's chip follows its runs, which `tasks:attempt` keeps fresh.
  const started = live.some((p) => p.state === "started");
  useEffect(() => {
    if (started) void useTasks.getState().loadAttempts();
  }, [started]);
  const STATUS = {
    planning: "working",
    failed: "error",
    ready: "waiting",
    created: "done",
    starting: "working",
    started: "working",
  } as const;
  return (
    <>
      {live.map((p) => (
        <Button
          key={p.id}
          size="sm"
          variant="ghost"
          onClick={() => usePlans.getState().open(p.id)}
          data-testid="plan-reopen"
        >
          <StatusDot
            status={
              p.state === "started" && read
                ? startedStatus(p, attempts)
                : STATUS[p.state as keyof typeof STATUS]
            }
          />
          <span className="max-w-48 truncate">{t("plan.chip", { goal: p.goal.split("\n")[0] ?? "" })}</span>
        </Button>
      ))}
    </>
  );
}

/** The full-page view (Agent mode's "Tasks"). */
export function TasksPage() {
  const [asking, setAsking] = useState(false);
  return (
    <TasksPanel
      header={
        <div className="flex h-11 shrink-0 items-center gap-2 border-b border-divider px-3">
          <h1 className="text-md font-semibold text-fg">{t("tasks.title")}</h1>
          <span className="flex-1" />
          <OpenPlans />
          <Button size="sm" variant="secondary" onClick={() => setAsking(true)} data-testid="plan-with-lead">
            {t("plan.open")}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => void useTasks.getState().load()}>
            {t("tasks.refresh")}
          </Button>
          <PlanWithLead asking={asking} onAskingChange={setAsking} />
        </div>
      }
    />
  );
}
