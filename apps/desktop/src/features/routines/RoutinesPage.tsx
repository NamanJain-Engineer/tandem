// UI-50 Routines (F-RT-04): the today strip, a row per routine with its
// schedule in words, next run, last result and actions, the background
// runner chip, and the empty, loading, error, Free and plan-lapsed states.
import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import { CalendarClock, MoreHorizontal, Pause, Play, Plus } from "lucide-react";
import type { RoutineView } from "@tandem/protocol";
import {
  Avatar,
  Badge,
  Banner,
  Button,
  ConfirmDialog,
  EmptyState,
  IconButton,
  Menu,
  Skeleton,
  StatusDot,
  Tooltip,
  useToast,
  type BadgeVariant,
} from "@tandem/ui";
import { t } from "../../i18n";
import { useAgentPage } from "../../stores/agentPage";
import { useRoutines } from "../../stores/routines";
import { useSession } from "../../stores/session";
import { useTeammates } from "../../stores/teammates";
import { useUi } from "../../stores/ui";
import { switchMode } from "../shell/actions";
import { RoutineEditor, type EditorSeed } from "./RoutineEditor";
import { RunHistory } from "./RunHistory";
import { describe } from "./schedule";

const DAY = 86_400_000;

function dayStart(now: number): number {
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** "in 21 hours" / "5 minutes ago". */
export function relative(ms: number, now: number): string {
  const diff = ms - now;
  const abs = Math.abs(diff);
  const fmt = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
  if (abs < 60_000) return fmt.format(Math.round(diff / 1000), "second");
  if (abs < 3_600_000) return fmt.format(Math.round(diff / 60_000), "minute");
  if (abs < DAY) return fmt.format(Math.round(diff / 3_600_000), "hour");
  return fmt.format(Math.round(diff / DAY), "day");
}

function exact(ms: number): string {
  return new Date(ms).toLocaleString("en-GB", {
    weekday: "short",
    day: "numeric",
    month: "short",
    hour: "numeric",
    minute: "2-digit",
    hour12: true,
  });
}

/** Opens a run's chat on its teammate's page. */
export function openChat(teammateId: string, chatId: string): void {
  switchMode("agent");
  useAgentPage.getState().show("teammate");
  const tm = useTeammates.getState();
  tm.selectChat(teammateId, chatId);
  tm.select(teammateId);
}

function useNow(every = 30_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), every);
    return () => window.clearInterval(id);
  }, [every]);
  return now;
}

/** The last result's badge (UI-50). */
function lastBadge(v: RoutineView, now: number): { variant: BadgeVariant; text: string } {
  const r = v.routine;
  const last = v.last;
  const when = last ? relative(last.endedAt ?? last.startedAt ?? last.dueAt, now) : "";
  if (v.running) return { variant: "working", text: t("routines.row.running") };
  switch (r.lastStatus) {
    case "ok":
      return { variant: "done", text: t("routines.row.ready", { when }) };
    case "error":
      return { variant: "error", text: t("routines.row.failed", { when }) };
    case "skipped":
      return { variant: "idle", text: t("routines.row.skipped", { when }) };
    default:
      return { variant: "neutral", text: t("routines.row.never") };
  }
}

/** UI-50 today strip: today's runs and what's still due, with a now line. */
function TodayStrip({ list, now }: { list: RoutineView[]; now: number }) {
  const start = dayStart(now);
  const at = (ms: number) => `${Math.min(100, Math.max(0, ((ms - start) / DAY) * 100))}%`;
  const marks: Array<{
    key: string;
    ms: number;
    kind: "done" | "failed" | "running" | "upcoming";
    label: string;
  }> = [];
  for (const v of list) {
    for (const run of v.today) {
      const kind =
        run.status === "ok"
          ? "done"
          : run.status === "running"
            ? "running"
            : run.status === "error"
              ? "failed"
              : null;
      if (!kind) continue;
      marks.push({
        key: run.id,
        ms: run.dueAt,
        kind,
        label: t("routines.strip.mark", {
          name: v.routine.name,
          time: new Date(run.dueAt).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" }),
          result: t(`routines.strip.${kind}`),
        }),
      });
    }
    const next = v.routine.nextRunAt;
    if (next && next >= now && next < start + DAY && !v.routine.paused) {
      marks.push({
        key: `${v.routine.id}-next`,
        ms: next,
        kind: "upcoming",
        label: t("routines.strip.mark", {
          name: v.routine.name,
          time: new Date(next).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" }),
          result: t("routines.strip.upcoming"),
        }),
      });
    }
  }
  const DOT = {
    done: "bg-done",
    failed: "bg-error",
    running: "bg-working",
    upcoming: "border border-fg-muted bg-surface-2",
  } as const;
  return (
    <div className="hidden px-4 pb-2 pt-1 min-[900px]:block" data-testid="routines-strip">
      <div className="flex justify-between text-xs text-fg-subtle" aria-hidden="true">
        <span>{t("routines.strip.today")}</span>
        {[0, 6, 12, 18, 24].map((h) => (
          <span key={h}>{String(h).padStart(2, "0")}</span>
        ))}
      </div>
      <div
        className="relative mt-1 h-4 rounded-sm bg-surface-3"
        role="group"
        aria-label={t("routines.strip.label")}
      >
        {marks.map((m) => (
          <Tooltip key={m.key} label={m.label}>
            <span
              role="img"
              aria-label={m.label}
              tabIndex={0}
              data-testid="routines-strip-mark"
              data-kind={m.kind}
              className={`absolute top-1/2 size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full ${DOT[m.kind]}`}
              style={{ left: at(m.ms) }}
            />
          </Tooltip>
        ))}
        <span
          className="absolute inset-y-0 w-0.5 bg-accent"
          style={{ left: at(now) }}
          data-testid="routines-now"
          role="img"
          aria-label={t("routines.strip.now", {
            time: new Date(now).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" }),
          })}
        />
      </div>
    </div>
  );
}

function BackgroundChip() {
  const background = useRoutines((s) => s.background);
  const limits = useSession((s) => s.entitlement?.limits);
  const { show: toast } = useToast();
  const set = async (on: boolean) => {
    const e = await useRoutines.getState().setBackground(on);
    if (e) toast({ tone: "error", title: e.message });
  };
  if (background?.installed) {
    return (
      <span
        className="flex items-center gap-2 text-sm text-fg-muted"
        data-testid="routines-background"
        data-on="true"
      >
        <StatusDot status="done" />
        {t("routines.background.on")}
        <Button
          size="sm"
          variant="ghost"
          onClick={() => void set(false)}
          data-testid="routines-background-off"
        >
          {t("routines.background.turnOff")}
        </Button>
      </span>
    );
  }
  return (
    <span
      className="flex items-center gap-2 text-sm text-fg-muted"
      data-testid="routines-background"
      data-on="false"
    >
      {t("routines.background.off")}
      {limits?.backgroundRoutines ? (
        <Button size="sm" variant="ghost" onClick={() => void set(true)} data-testid="routines-background-on">
          {t("routines.background.turnOn")}
        </Button>
      ) : (
        <Badge variant="accent">{t("routines.pro")}</Badge>
      )}
    </span>
  );
}

type RowProps = {
  v: RoutineView;
  now: number;
  onEdit: () => void;
  onHistory: () => void;
  onDelete: () => void;
};

function Row({ v, now, onEdit, onHistory, onDelete }: RowProps) {
  const r = v.routine;
  const tm = useTeammates((s) => s.list.find((x) => x.teammate.id === r.teammateId)?.teammate ?? null);
  const maker = useTeammates(
    (s) => s.list.find((x) => x.teammate.id === r.createdByTeammateId)?.teammate.name ?? null,
  );
  const { show: toast } = useToast();
  const badge = lastBadge(v, now);
  const words = v.words ? describe(v.words) : t("routines.row.unreadable");
  const run = async () => {
    const e = await useRoutines.getState().runNow(r.id);
    if (e) toast({ tone: "error", title: e.message });
  };
  const pause = async () => {
    const e = await useRoutines.getState().pause(r.id, !r.paused);
    if (e) toast({ tone: "error", title: e.message });
  };
  // A run in progress makes the core refuse with "previous run still in
  // progress", which the toast says.
  const runNow = () => void run();
  const onKeyDown = (e: KeyboardEvent<HTMLLIElement>) => {
    if (e.target !== e.currentTarget) return;
    const k = e.key.toLowerCase();
    if (k === "enter") onEdit();
    else if (k === " ") runNow();
    else if (k === "p") void pause();
    else if (k === "h") onHistory();
    else if (k === "delete") onDelete();
    else return;
    e.preventDefault();
  };
  const status = v.running ? "working" : r.lastStatus === "error" ? "error" : r.paused ? "idle" : "done";
  const fix =
    r.lastErrorKind === "teammate_missing" ? (
      <Button size="sm" variant="secondary" onClick={onEdit}>
        {t("routines.row.chooseTeammate")}
      </Button>
    ) : (
      <Button size="sm" variant="secondary" onClick={runNow}>
        {t("routines.row.runNow")}
      </Button>
    );
  const lastChat = r.lastRunChatId;
  return (
    <li
      tabIndex={0}
      onKeyDown={onKeyDown}
      className="flex flex-col gap-1 border-b border-divider px-4 py-2 outline-none last:border-b-0 focus-visible:shadow-focus"
      data-testid="routine-row"
      data-id={r.id}
      data-status={r.lastStatus ?? "never"}
      data-running={v.running ? "true" : "false"}
      data-paused={r.paused || r.pausedByPlan ? "true" : "false"}
    >
      <div className="flex flex-wrap items-center gap-3">
        {r.paused ? (
          <span className="inline-flex text-fg-muted" role="img" aria-label={t("routines.row.paused")}>
            <Pause size={10} aria-hidden="true" />
          </span>
        ) : (
          <StatusDot status={status} label={badge.text} />
        )}
        <span className="min-w-0 flex-1 truncate text-base font-medium text-fg" data-testid="routine-name">
          {r.name}
        </span>
        {r.createdByTeammateId && (
          <span className="text-sm text-fg-muted" data-testid="routine-created-by">
            {t("routines.row.createdBy", { name: maker ?? t("routines.row.aTeammate") })}
          </span>
        )}
        {r.access === "full" && <Badge variant="waiting">{t("routines.row.full")}</Badge>}
        <span className="flex items-center gap-1.5 text-sm text-fg-muted">
          {tm ? (
            <>
              <Avatar name={tm.name} id={tm.id} hue={tm.avatarHue} size={20} />
              {tm.name}
            </>
          ) : (
            t("routines.row.noTeammate")
          )}
        </span>
        <span className="text-sm text-fg-muted" data-testid="routine-words">
          {words}
        </span>
        <span className="text-sm text-fg-muted" data-testid="routine-next">
          {r.paused || r.pausedByPlan ? (
            t("routines.row.paused")
          ) : r.nextRunAt ? (
            <Tooltip label={exact(r.nextRunAt)}>
              <span tabIndex={-1}>{t("routines.row.next", { when: relative(r.nextRunAt, now) })}</span>
            </Tooltip>
          ) : (
            "—"
          )}
        </span>
        <button
          type="button"
          onClick={onHistory}
          className="rounded-sm outline-none focus-visible:shadow-focus"
          aria-label={t("routines.row.historyOf", { name: r.name })}
        >
          <Badge variant={badge.variant}>
            <span data-testid="routine-last">{badge.text}</span>
          </Badge>
        </button>
        <IconButton
          size="sm"
          icon={<Play size={14} />}
          label={t("routines.row.runNow")}
          onClick={runNow}
          data-testid="routine-run-now"
        />
        <Menu
          align="end"
          trigger={
            <IconButton
              size="sm"
              icon={<MoreHorizontal size={14} />}
              label={t("routines.row.menu", { name: r.name })}
              data-testid="routine-menu"
            />
          }
          entries={[
            { id: "edit", label: t("routines.row.edit"), kbd: ["Enter"], onSelect: onEdit },
            {
              id: "pause",
              label: r.paused ? t("routines.row.resume") : t("routines.row.pause"),
              kbd: ["P"],
              onSelect: () => void pause(),
            },
            {
              id: "open",
              label: t("routines.row.openLast"),
              disabled: !lastChat || !r.teammateId,
              onSelect: () => lastChat && r.teammateId && openChat(r.teammateId, lastChat),
            },
            { id: "history", label: t("routines.row.history"), kbd: ["H"], onSelect: onHistory },
            { id: "delete", label: t("routines.row.delete"), danger: true, onSelect: onDelete },
          ]}
        />
      </div>
      {r.lastStatus === "error" && r.lastError && (
        <div className="flex flex-wrap items-center gap-2 pl-5 text-sm" data-testid="routine-error">
          <span className="text-error">{r.lastError}</span>
          {fix}
          {lastChat && r.teammateId && (
            <Button size="sm" variant="ghost" onClick={() => openChat(r.teammateId!, lastChat)}>
              {t("routines.row.openRun")}
            </Button>
          )}
        </div>
      )}
    </li>
  );
}

/** Suggestion chips for the empty state, pre-filling the editor. */
const SUGGESTIONS = ["summary", "update", "deps"] as const;

export function RoutinesPage() {
  const { list, status, error } = useRoutines();
  const limits = useSession((s) => s.entitlement?.limits);
  const signedIn = useSession((s) => s.auth?.signedIn ?? false);
  const now = useNow();
  const { show: toast } = useToast();
  const [editing, setEditing] = useState<EditorSeed | null>(null);
  const [history, setHistory] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<RoutineView | null>(null);
  useEffect(() => {
    void useRoutines.getState().load();
    void useTeammates.getState().refresh();
  }, []);
  const sorted = useMemo(
    () => [...list].sort((a, b) => (a.routine.nextRunAt ?? Infinity) - (b.routine.nextRunAt ?? Infinity)),
    [list],
  );
  const pro = !!limits?.routines;
  const lapsed = !pro && list.some((v) => v.routine.pausedByPlan);
  const historyOf = list.find((v) => v.routine.id === history) ?? null;
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const tag = (e.target as HTMLElement).tagName;
    if (tag === "INPUT" || tag === "TEXTAREA") return;
    if (e.key.toLowerCase() === "n" && !e.metaKey && !e.ctrlKey && !e.altKey && pro) {
      e.preventDefault();
      setEditing({});
    }
  };
  const remove = async (v: RoutineView) => {
    const e = await useRoutines.getState().remove(v.routine.id);
    if (e) toast({ tone: "error", title: e.message });
  };
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="routines-page" onKeyDown={onKeyDown}>
      <div className="flex shrink-0 flex-wrap items-center gap-3 border-b border-divider px-4 py-2">
        <div className="flex min-w-0 flex-1 flex-col">
          <h1 className="text-md font-semibold text-fg">{t("routines.title")}</h1>
          <p className="text-sm text-fg-muted">{t("routines.subtitle")}</p>
        </div>
        {pro && <BackgroundChip />}
        {pro && (
          <Button
            size="sm"
            variant="primary"
            iconLeft={<Plus size={14} />}
            onClick={() => setEditing({})}
            data-testid="routine-new"
          >
            {t("routines.new")}
          </Button>
        )}
      </div>
      {lapsed && <Banner tone="warning">{t("routines.lapsed")}</Banner>}
      {pro && list.length > 0 && <TodayStrip list={list} now={now} />}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {!pro && !lapsed ? (
          <EmptyState
            icon={<CalendarClock size={20} />}
            title={t("routines.upsell.title")}
            body={signedIn ? t("routines.upsell.body") : t("routines.upsell.signIn")}
            primary={
              signedIn ? (
                <Button
                  variant="primary"
                  onClick={() => useUi.getState().openUpgrade()}
                  data-testid="routines-upgrade"
                >
                  {t("routines.upsell.upgrade")}
                </Button>
              ) : undefined
            }
            className="[&]:max-w-[480px]"
          />
        ) : status === "loading" || status === "idle" ? (
          <div className="flex flex-col gap-2 p-4" data-testid="routines-loading">
            {[0, 1, 2].map((i) => (
              <Skeleton key={i} height={36} />
            ))}
          </div>
        ) : status === "error" ? (
          <EmptyState
            title={t("routines.error")}
            body={error ?? undefined}
            primary={
              <Button variant="secondary" onClick={() => void useRoutines.getState().load()}>
                {t("routines.retry")}
              </Button>
            }
          />
        ) : list.length === 0 ? (
          <EmptyState
            icon={<CalendarClock size={20} />}
            title={t("routines.empty.title")}
            body={t("routines.empty.body")}
            primary={
              <Button variant="primary" onClick={() => setEditing({})}>
                {t("routines.new")}
              </Button>
            }
            secondary={
              <span className="flex flex-wrap justify-center gap-2" data-testid="routine-suggestions">
                {SUGGESTIONS.map((k) => (
                  <Button
                    key={k}
                    size="sm"
                    variant="secondary"
                    onClick={() =>
                      setEditing({
                        name: t(`routines.suggest.${k}.name`),
                        instruction: t(`routines.suggest.${k}.instruction`),
                      })
                    }
                  >
                    {t(`routines.suggest.${k}.name`)}
                  </Button>
                ))}
              </span>
            }
          />
        ) : (
          <ul aria-label={t("routines.title")} data-testid="routines-list">
            {sorted.map((v) => (
              <Row
                key={v.routine.id}
                v={v}
                now={now}
                onEdit={() => setEditing({ routine: v.routine })}
                onHistory={() => setHistory(v.routine.id)}
                onDelete={() => setDeleting(v)}
              />
            ))}
          </ul>
        )}
      </div>
      {editing && <RoutineEditor seed={editing} onClose={() => setEditing(null)} />}
      {historyOf && <RunHistory view={historyOf} onClose={() => setHistory(null)} />}
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => !o && setDeleting(null)}
        title={t("routines.delete.title", { name: deleting?.routine.name ?? "" })}
        body={t("routines.delete.body")}
        confirmLabel={t("routines.delete.confirm")}
        cancelLabel={t("routines.cancel")}
        danger
        onConfirm={() => {
          if (deleting) void remove(deleting);
          setDeleting(null);
        }}
      />
    </div>
  );
}
