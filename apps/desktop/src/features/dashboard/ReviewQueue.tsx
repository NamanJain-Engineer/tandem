import { useEffect, useMemo, useState } from "react";
import { ClipboardCheck } from "lucide-react";
import type { FileDiff, IpcError, RepoReview, ReviewItem, ReviewRiskRule } from "@tandem/protocol";
import {
  Badge,
  Button,
  Dialog,
  DiffView,
  EmptyState,
  Input,
  SegmentedControl,
  Skeleton,
  Textarea,
  useDelayedFlag,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useDashboard } from "../../stores/dashboard";
import { useEngines } from "../../stores/engines";
import { focusSource } from "../../stores/focus";
import { diffNames } from "../../stores/intel";
import { sortItems, toReview, useReview } from "../../stores/review";
import { useSidebar } from "../../stores/sidebar";
import { useWorktrees } from "../../stores/worktrees";
import { Dashboard } from "./Dashboard";

const RULE: Record<ReviewRiskRule, () => string> = {
  many_files: () => t("review.rule.manyFiles"),
  migrations: () => t("review.rule.migrations"),
  ci: () => t("review.rule.ci"),
  lockfile: () => t("review.rule.lockfile"),
  env: () => t("review.rule.env"),
  auth: () => t("review.rule.auth"),
  public_api: () => t("review.rule.publicApi"),
  deleted_tests: () => t("review.rule.deletedTests"),
};

export function ruleLabel(rule: ReviewRiskRule): string {
  return RULE[rule]();
}

function ago(at: number, now: number): string {
  const m = Math.max(0, Math.floor((now - at) / 60_000));
  return m < 60 ? t("time.minutes", { n: m }) : t("time.hoursMinutes", { h: Math.floor(m / 60), m: m % 60 });
}

function act(p: Promise<unknown>, show: ReturnType<typeof useToast>["show"]) {
  void p.catch((e: IpcError) => show({ tone: "error", title: e.message }));
}

/** The diff of an item: all of it, or since the agent's last checkpoint. */
function ItemDiff({
  item,
  scope,
  onClose,
}: {
  item: ReviewItem;
  scope: "all" | "turn";
  onClose: () => void;
}) {
  const [files, setFiles] = useState<FileDiff[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    void invoke<FileDiff[]>("review_item_diff", { key: item.key, scope }).then(setFiles, (e: IpcError) =>
      setError(e.message),
    );
  }, [item.key, scope]);
  return (
    <Dialog
      open
      size="lg"
      onOpenChange={(o) => !o && onClose()}
      title={
        scope === "turn"
          ? t("review.turnTitle", { name: item.title })
          : t("review.diffTitle", { name: item.title })
      }
      closeLabel={t("common.close")}
    >
      <div data-testid="review-diff" className="max-h-[60vh] overflow-y-auto">
        {error ? (
          <p className="text-sm text-error">{error}</p>
        ) : files === null ? (
          <Skeleton height={96} />
        ) : (
          <DiffView
            files={files}
            labels={{ empty: t("worktree.diffEmpty") }}
            names={diffNames({ type: "workspace", workspaceId: item.laneId ?? item.workspaceId }, onClose)}
          />
        )}
      </div>
    </Dialog>
  );
}

/** The repository's check command and public API paths, on this computer. */
function RepoSettings({ workspaceId, onClose }: { workspaceId: string; onClose: () => void }) {
  const { show } = useToast();
  const [repo, setRepo] = useState<RepoReview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [check, setCheck] = useState("");
  const [paths, setPaths] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    void invoke<RepoReview>("review_repo_get", { workspaceId }).then(
      (r) => {
        setRepo(r);
        setCheck(r.check ?? "");
        setPaths(r.apiPaths.join("\n"));
      },
      (e: IpcError) => setError(e.message),
    );
  }, [workspaceId]);
  const save = async () => {
    setBusy(true);
    try {
      await invoke<RepoReview>("review_repo_set", {
        workspaceId,
        check: check.trim() || null,
        apiPaths: paths
          .split("\n")
          .map((p) => p.trim())
          .filter(Boolean),
      });
      show({ tone: "success", title: t("review.repo.saved") });
      onClose();
    } catch (e) {
      setError((e as IpcError).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("review.repo.title")}
      description={repo ? repo.root : undefined}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={!repo}
            onClick={() => void save()}
            data-testid="review-repo-save"
          >
            {t("review.repo.save")}
          </Button>
        </>
      }
    >
      {error ? (
        <p className="text-sm text-error" data-testid="review-repo-error">
          {error}
        </p>
      ) : !repo ? (
        <Skeleton height={64} />
      ) : (
        <div className="flex flex-col gap-3">
          <Input
            label={t("review.repo.check")}
            placeholder="pnpm test"
            value={check}
            maxLength={2000}
            onChange={(e) => setCheck(e.target.value)}
            data-testid="review-repo-check"
          />
          <p className="text-xs text-fg-muted">{t("review.repo.checkHelp")}</p>
          <Textarea
            label={t("review.repo.apiPaths")}
            minRows={2}
            maxRows={6}
            value={paths}
            onChange={(e) => setPaths(e.target.value)}
            data-testid="review-repo-paths"
          />
        </div>
      )}
    </Dialog>
  );
}

function ReviewCard({ item, now }: { item: ReviewItem; now: number }) {
  const { show } = useToast();
  const engine = useEngines((s) => s.label(item.engineId));
  const [diff, setDiff] = useState<"all" | "turn" | null>(null);
  const [repo, setRepo] = useState(false);
  const open = useWorktrees.getState().open;
  const lane = item.laneId;
  const approved = item.state === "approved";
  return (
    <article
      data-testid="review-item"
      data-key={item.key}
      data-state={item.state}
      className="flex flex-col gap-2 rounded-md border border-border bg-surface-2 p-3"
    >
      <header className="flex min-w-0 flex-wrap items-center gap-2">
        <span className="min-w-0 flex-1 truncate text-sm font-medium text-fg" data-testid="review-item-title">
          {item.title}
        </span>
        {item.state === "changed" && (
          <Badge variant="waiting">
            <span data-testid="review-changed">{t("review.changed")}</span>
          </Badge>
        )}
        {approved && <Badge variant="done">{t("review.approvedBadge")}</Badge>}
      </header>
      <p className="truncate text-xs text-fg-muted">
        {[engine, item.workspace, item.branch, ago(item.finishedAt, now)].filter(Boolean).join(" · ")}
      </p>
      {item.closing && (
        <p className="line-clamp-3 whitespace-pre-wrap text-sm text-fg" data-testid="review-closing">
          {item.closing}
        </p>
      )}
      <p className="text-xs text-fg-muted" data-testid="review-stat">
        {t("review.stat", { n: item.files, adds: item.additions, dels: item.deletions })}
      </p>
      {item.risks.length > 0 && (
        <ul className="flex flex-wrap gap-1.5" aria-label={t("review.risks")}>
          {item.risks.map((r) => (
            <li key={r.rule} data-testid="review-risk" data-rule={r.rule} title={r.files.join("\n")}>
              <Badge variant="outline">{ruleLabel(r.rule)}</Badge>
            </li>
          ))}
        </ul>
      )}
      {item.check && (
        <details data-testid="review-check" data-state={item.check.state} className="text-xs">
          <summary className="cursor-default text-fg-muted">
            {item.check.state === "running"
              ? t("review.check.running")
              : item.check.state === "passed"
                ? t("review.check.passed")
                : t("review.check.failed", { code: item.check.exit ?? "—" })}
          </summary>
          {item.check.output && (
            <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded-sm bg-surface-1 p-2 text-fg">
              {item.check.output}
            </pre>
          )}
        </details>
      )}
      <div className="flex flex-wrap gap-1.5">
        <Button
          size="sm"
          variant={approved ? "ghost" : "primary"}
          onClick={() =>
            act(
              approved
                ? invoke("review_unapprove", { key: item.key })
                : invoke("review_approve", { key: item.key, diffHash: item.diffHash }),
              show,
            )
          }
          data-testid="review-approve"
        >
          {approved ? t("review.unapprove") : t("review.approve")}
        </Button>
        <Button size="sm" variant="secondary" onClick={() => setDiff("all")} data-testid="review-open-diff">
          {t("review.openDiff")}
        </Button>
        {item.turnCheckpointId && (
          <Button size="sm" variant="ghost" onClick={() => setDiff("turn")} data-testid="review-turn">
            {t("review.lastTurn")}
          </Button>
        )}
        {lane && (
          <>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => open({ kind: "diff", worktreeId: lane })}
              data-testid="review-send-back"
            >
              {t("review.sendBack")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => open({ kind: "opinion", worktreeId: lane })}
              data-testid="review-opinion"
            >
              {t("review.secondOpinion")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => open({ kind: "merge", worktreeId: lane })}
              data-testid="review-merge"
            >
              {t("review.merge")}
            </Button>
          </>
        )}
        {item.check && item.check.state !== "running" && (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => act(invoke("review_run_check", { key: item.key }), show)}
            data-testid="review-run-check"
          >
            {t("review.runCheck")}
          </Button>
        )}
        {item.state !== "snoozed" ? (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => act(invoke("review_snooze", { key: item.key }), show)}
            data-testid="review-snooze"
          >
            {t("review.snooze")}
          </Button>
        ) : (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => act(invoke("review_snooze", { key: item.key, minutes: 0 }), show)}
            data-testid="review-unsnooze"
          >
            {t("review.unsnooze")}
          </Button>
        )}
        <Button
          size="sm"
          variant="ghost"
          onClick={() => void focusSource(item.source)}
          data-testid="review-jump"
        >
          {t("review.jump")}
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setRepo(true)} data-testid="review-repo">
          {t("review.repo.menu")}
        </Button>
      </div>
      {diff && <ItemDiff item={item} scope={diff} onClose={() => setDiff(null)} />}
      {repo && <RepoSettings workspaceId={item.workspaceId} onClose={() => setRepo(false)} />}
    </article>
  );
}

function Section({ title, items, now }: { title: string; items: ReviewItem[]; now: number }) {
  if (items.length === 0) return null;
  return (
    <section className="flex flex-col gap-2" aria-label={title}>
      <h3 className="text-xs font-semibold uppercase tracking-caps text-fg-muted">
        {title} · {items.length}
      </h3>
      {items.map((i) => (
        <ReviewCard key={i.key} item={i} now={now} />
      ))}
    </section>
  );
}

/** F-DB-04, the Review view: what's ready to review, across engines. */
export function ReviewQueue() {
  const { items, loaded, error, sort } = useReview();
  const slow = useDelayedFlag(!loaded, 150);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    // Opening the queue refreshes every item's diff (an approval may be stale).
    void useReview.getState().load(true);
    const id = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(id);
  }, []);
  const sorted = useMemo(() => sortItems(items, sort), [items, sort]);

  if (error)
    return (
      <div className="flex flex-col items-start gap-2 p-3" data-testid="review-error">
        <p className="text-sm text-error">{error}</p>
        <Button size="sm" variant="secondary" onClick={() => void useReview.getState().load(true)}>
          {t("common.retry")}
        </Button>
      </div>
    );
  if (!loaded)
    return slow ? (
      <div className="flex flex-col gap-2 p-3" data-testid="review-loading">
        {[0, 1].map((n) => (
          <Skeleton key={n} height={96} className="rounded-md" />
        ))}
      </div>
    ) : null;
  if (items.length === 0)
    return (
      <div className="p-3" data-testid="review-empty">
        <EmptyState
          icon={<ClipboardCheck size={20} />}
          title={t("review.emptyTitle")}
          body={t("review.emptyBody")}
        />
      </div>
    );
  return (
    <div className="flex flex-col gap-3 p-3" data-testid="review-queue">
      <div className="flex items-center gap-2">
        <SegmentedControl
          size="sm"
          label={t("review.sortBy")}
          value={sort}
          onValueChange={(v) => useReview.getState().setSort(v === "time" ? "time" : "risk")}
          items={[
            { id: "risk", label: t("review.byRisk") },
            { id: "time", label: t("review.byTime") },
          ]}
        />
        <span className="flex-1" />
        <Button
          size="sm"
          variant="ghost"
          onClick={() => void useReview.getState().load(true)}
          data-testid="review-refresh"
        >
          {t("review.refresh")}
        </Button>
      </div>
      <Section title={t("review.toReview")} items={toReview(sorted)} now={now} />
      <Section title={t("review.snoozed")} items={sorted.filter((i) => i.state === "snoozed")} now={now} />
      <Section title={t("review.approved")} items={sorted.filter((i) => i.state === "approved")} now={now} />
    </div>
  );
}

/** The Dashboard panel: Agents (F-DB-01) or Review (F-DB-04). */
export function DashboardPanel() {
  const view = useReview((s) => s.view);
  const pending = useReview((s) => toReview(s.items).length);
  const agents = useDashboard((s) => Object.keys(s.items).length);
  const fullscreen = useSidebar((s) => s.fullscreen);
  useEffect(() => {
    // The count shows before the view is opened.
    if (!useReview.getState().loaded) void useReview.getState().load(false);
  }, []);
  return (
    <div className="flex min-h-0 flex-col" data-fullscreen={fullscreen || undefined}>
      <div className="px-3 pt-3">
        <SegmentedControl
          size="sm"
          label={t("review.view")}
          value={view}
          onValueChange={(v) => useReview.getState().setView(v === "review" ? "review" : "agents")}
          items={[
            { id: "agents", label: t("review.viewAgents", { n: agents }) },
            { id: "review", label: t("review.viewReview", { n: pending }) },
          ]}
        />
      </div>
      {view === "review" ? <ReviewQueue /> : <Dashboard />}
    </div>
  );
}
