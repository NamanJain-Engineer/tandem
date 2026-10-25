import { useCallback, useEffect, useState } from "react";
import type { ActivityEvent, IpcError } from "@tandem/protocol";
import {
  Bot,
  CheckCircle2,
  FileDiff,
  KeyRound,
  MessageSquare,
  ShieldQuestion,
  SquareTerminal,
  Wrench,
} from "lucide-react";
import { Button, Callout, Dialog, EmptyState, Skeleton, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { activityExport, activityList, useActivity } from "../../stores/activity";
import { focusSource } from "../../stores/focus";
import { useSidebar } from "../../stores/sidebar";

type Load = { at: "loading" } | { at: "error"; message: string } | { at: "ready"; events: ActivityEvent[] };

const ICONS: Record<string, typeof Bot> = {
  turn: MessageSquare,
  files: FileDiff,
  command: SquareTerminal,
  tool: Wrench,
  subagent: Bot,
  approval: ShieldQuestion,
  run: KeyRound,
};

function time(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** Who decided a run or an approval, in words. */
export function decidedLabel(decided: string | null): string | null {
  if (!decided) return null;
  const [state, via] = decided.split(":");
  switch (state) {
    case "once":
      return t("activity.decided.once");
    case "agent":
      return t("activity.decided.agent");
    case "rule":
      return t("activity.decided.rule");
    case "allowed_before":
      return t("activity.decided.before");
    case "declined":
    case "denied":
      return via ? t("activity.decided.deniedVia", { via: viaLabel(via) }) : t("activity.decided.declined");
    case "allowed":
      return t("activity.decided.allowedVia", { via: viaLabel(via ?? "") });
    case "expired":
      return t("activity.decided.expired");
    case "cancelled":
      return t("activity.decided.cancelled");
    case "pending":
      return t("activity.decided.pending");
    default:
      return state ?? null;
  }
}

function viaLabel(via: string): string {
  switch (via) {
    case "inline":
      return t("activity.via.inline");
    case "dashboard":
      return t("activity.via.dashboard");
    case "notification":
      return t("activity.via.notification");
    case "phone":
      return t("activity.via.phone");
    case "voice":
      return t("activity.via.voice");
    case "rule":
      return t("activity.via.rule");
    default:
      return via;
  }
}

/** The event's one line, then its facts ("exit 0 · 840 ms · uses stripe-test · You, once"). */
function facts(e: ActivityEvent): string[] {
  const out: string[] = [];
  if (e.exitCode !== null) out.push(t("activity.exit", { code: e.exitCode }));
  if (e.durationMs !== null) out.push(t("activity.took", { ms: e.durationMs }));
  if (e.secrets.length > 0) out.push(t("activity.uses", { names: e.secrets.join(", ") }));
  const who = decidedLabel(e.decided);
  if (who) out.push(who);
  return out;
}

function kindLabel(kind: string): string {
  switch (kind) {
    case "turn":
      return t("activity.kind.turn");
    case "files":
      return t("activity.kind.files");
    case "command":
      return t("activity.kind.command");
    case "tool":
      return t("activity.kind.tool");
    case "subagent":
      return t("activity.kind.subagent");
    case "approval":
      return t("activity.kind.approval");
    case "run":
      return t("activity.kind.run");
    default:
      return kind;
  }
}

function Row({
  e,
  onCheckpoint,
  onTranscript,
}: {
  e: ActivityEvent;
  onCheckpoint: () => void;
  onTranscript?: () => void;
}) {
  const Icon = ICONS[e.kind] ?? CheckCircle2;
  const more = facts(e);
  return (
    <li
      className="flex items-start gap-2 border-b border-divider py-1.5 text-sm last:border-b-0"
      data-testid="activity-row"
      data-kind={e.kind}
    >
      <span className="w-[70px] shrink-0 pt-0.5 font-mono text-xs tabular-nums text-fg-subtle">
        {time(e.at)}
      </span>
      <Icon size={14} aria-hidden="true" className="mt-0.5 shrink-0 text-fg-muted" />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <p className="flex min-w-0 items-baseline gap-2">
          <span className="shrink-0 text-xs font-semibold text-fg-muted">{kindLabel(e.kind)}</span>
          <span
            className={
              e.kind === "command" || e.kind === "run" ? "truncate font-mono text-fg" : "truncate text-fg"
            }
            title={e.summary}
            data-testid="activity-summary"
          >
            {e.summary}
          </span>
        </p>
        {more.length > 0 && (
          <p className="text-xs text-fg-muted" data-testid="activity-facts">
            {more.join(" · ")}
          </p>
        )}
      </div>
      <span className="flex shrink-0 gap-1">
        {onTranscript && e.kind === "turn" && (
          <Button size="sm" variant="ghost" onClick={onTranscript}>
            {t("activity.transcript")}
          </Button>
        )}
        {e.checkpointId && (
          <Button size="sm" variant="ghost" data-testid="activity-checkpoint" onClick={onCheckpoint}>
            {t("activity.checkpoint")}
          </Button>
        )}
      </span>
    </li>
  );
}

/** F-EX-07: a session's activity, oldest first, with Export. */
export function ActivityDialog() {
  const open = useActivity((s) => s.open);
  const { show } = useToast();
  const [load, setLoad] = useState<Load>({ at: "loading" });
  const [exporting, setExporting] = useState(false);
  const refresh = useCallback(() => {
    if (!open) return;
    setLoad({ at: "loading" });
    activityList(open)
      .then((events) => setLoad({ at: "ready", events }))
      .catch((e: IpcError) => setLoad({ at: "error", message: e.message ?? String(e) }));
  }, [open]);
  useEffect(refresh, [refresh]);
  if (!open) return null;
  const doExport = async () => {
    setExporting(true);
    try {
      const path = await activityExport(open);
      if (path) show({ title: t("activity.exported"), body: path });
    } catch (e) {
      show({ tone: "error", title: (e as IpcError).message ?? t("activity.exportFailed") });
    } finally {
      setExporting(false);
    }
  };
  const close = () => useActivity.getState().close();
  const toCheckpoint = () => {
    useSidebar.getState().openTab("checkpoints");
    close();
  };
  const toTranscript =
    open.source === "thread"
      ? () => {
          void focusSource({ type: "thread", threadId: open.id });
          close();
        }
      : undefined;
  const events = load.at === "ready" ? load.events : [];
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && close()}
      title={t("activity.title", { name: open.title })}
      size="lg"
      footer={
        <>
          <Button variant="ghost" onClick={close}>
            {t("common.close")}
          </Button>
          <Button
            variant="primary"
            data-testid="activity-export"
            loading={exporting}
            disabled={events.length === 0}
            onClick={() => void doExport()}
          >
            {t("activity.export")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-2" data-testid="activity-dialog">
        <p className="text-sm text-fg-muted">{t("activity.body")}</p>
        {load.at === "loading" ? (
          <div className="flex flex-col gap-1.5" data-testid="activity-loading">
            <Skeleton className="h-6" />
            <Skeleton className="h-6" />
            <Skeleton className="h-6" />
          </div>
        ) : load.at === "error" ? (
          <Callout
            tone="error"
            actions={
              <Button size="sm" variant="secondary" onClick={refresh}>
                {t("secrets.retry")}
              </Button>
            }
          >
            <span data-testid="activity-error">{t("activity.error")}</span>
          </Callout>
        ) : events.length === 0 ? (
          <EmptyState title={t("activity.empty.title")} body={t("activity.empty.body")} />
        ) : (
          <ol className="flex max-h-[60vh] flex-col overflow-y-auto" aria-label={t("activity.list")}>
            {events.map((e, i) => (
              <Row key={`${e.at}-${i}`} e={e} onCheckpoint={toCheckpoint} onTranscript={toTranscript} />
            ))}
          </ol>
        )}
      </div>
    </Dialog>
  );
}
