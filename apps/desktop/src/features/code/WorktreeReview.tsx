import { useMemo, useState } from "react";
import { create } from "zustand";
import type {
  DashboardItem,
  FileDiff,
  ImportedComment,
  ImportedComments,
  IpcError,
  SecondOpinion,
  WorkspaceSummary,
} from "@tandem/protocol";
import {
  Button,
  DiffView,
  Select,
  Spinner,
  Textarea,
  diffLineKey,
  useToast,
  type DiffLineTarget,
  type DiffViewLabels,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useDashboard } from "../../stores/dashboard";
import { useWorktrees } from "../../stores/worktrees";
import { diffNames } from "../../stores/intel";
import { useEngines } from "../../stores/engines";
import { switchMode } from "../shell/actions";
import { useThreads } from "../../stores/threads";
import { DEFANGED, defangMentions } from "./untrusted";

/** One review comment on a worktree's diff (F-WT-02). A General one (a
 * second opinion's lines that aren't on a shown line, F-WT-05) has no line.
 * `imported` when it came from the pull request (F-WT-08): someone else's
 * text, so the prompt carries the core's fenced copy, not the words.
 * `opinion` when the second opinion wrote it: another engine's words. */
export type ReviewComment = DiffLineTarget & {
  id: number;
  comment: string;
  general?: boolean;
  imported?: { id: string; author: string; fenced: string };
  opinion?: boolean;
};

const QUOTE_MAX = 200;

/** Path order, then the line, new side before old on the same number. */
function byPlace(a: ReviewComment, b: ReviewComment): number {
  if (!!a.general !== !!b.general) return a.general ? 1 : -1;
  if (a.general) return a.id - b.id;
  if (a.path !== b.path) return a.path < b.path ? -1 : 1;
  if (a.line !== b.line) return a.line - b.line;
  return a.side === b.side ? a.id - b.id : a.side === "new" ? -1 : 1;
}

function quote(text: string): string {
  const q = text.trim();
  return q.length > QUOTE_MAX ? `${q.slice(0, QUOTE_MAX - 1)}…` : q;
}

/** The one prompt "Send to agent" types (F-WT-02). The words are the
 * agent's instructions, so they stay in English like the app's other
 * prompts to engines. The paths and lines are the worktree agent's, and the
 * second opinion's words another engine's: no `@` in them names a file for
 * the agent to attach (D-3023). Your own comments stay as you wrote them. */
export function buildReviewPrompt(branch: string, comments: ReviewComment[]): string {
  const sorted = [...comments].sort(byPlace);
  const n = sorted.length;
  const head = `Review of your changes on ${branch} (${n} ${n === 1 ? "comment" : "comments"}):`;
  const indent = (text: string) =>
    text
      .split("\n")
      .map((l) => `   ${l}`)
      .join("\n");
  let defanged = false;
  const safe = (text: string) => {
    const out = defangMentions(text);
    defanged ||= out !== text;
    return out;
  };
  const items = sorted.map((c, i) => {
    // An imported comment's place and words are GitHub's: the core's
    // fenced copy carries both, and nothing of it goes outside the fence.
    if (c.imported) return `${i + 1}. Imported review comment:\n${indent(c.imported.fenced)}`;
    const where = c.general
      ? "General"
      : c.side === "new"
        ? `${safe(c.path)}:${c.line}`
        : `${safe(c.path)} (removed line ${c.line})`;
    const q = safe(quote(c.text));
    const words = c.opinion ? safe(c.comment.trim()) : c.comment.trim();
    return `${i + 1}. ${where}\n${q ? `   > ${q}\n` : ""}${indent(words)}`;
  });
  const note = defanged ? `${DEFANGED}\n\n` : "";
  return `${head}\n\n${items.join("\n\n")}\n\n${note}Address each comment, then say what you changed.`;
}

type ReviewsState = {
  /** Comments per worktree, kept until sent or the app quits (D-2205). */
  byLane: Record<string, ReviewComment[]>;
  next: number;
  add: (laneId: string, target: DiffLineTarget, comment: string) => void;
  edit: (laneId: string, id: number, comment: string) => void;
  remove: (laneId: string, id: number) => void;
  clear: (laneId: string) => void;
  /** F-WT-08: pull request comment ids each worktree has imported, sent
   * or removed since: importing again never brings them back. */
  importedIds: Record<string, string[]>;
  /** F-WT-08: the pull request's comments not imported yet; how many. */
  addImported: (laneId: string, list: ImportedComment[]) => number;
  /** F-WT-05: a second opinion's comments, after the user's own. */
  addOpinion: (op: SecondOpinion) => void;
  /** F-WT-05: the reviewer running per worktree. */
  reviewing: Record<string, { threadId: string; engineId: string }>;
  setReviewing: (laneId: string, r: { threadId: string; engineId: string } | null) => void;
};

export const useReviews = create<ReviewsState>((set, get) => ({
  byLane: {},
  next: 1,
  importedIds: {},
  add: (laneId, target, comment) => {
    const id = get().next;
    const list = get().byLane[laneId] ?? [];
    set({ next: id + 1, byLane: { ...get().byLane, [laneId]: [...list, { ...target, id, comment }] } });
  },
  edit: (laneId, id, comment) =>
    set({
      byLane: {
        ...get().byLane,
        [laneId]: (get().byLane[laneId] ?? []).map((c) => (c.id === id ? { ...c, comment } : c)),
      },
    }),
  remove: (laneId, id) =>
    set({ byLane: { ...get().byLane, [laneId]: (get().byLane[laneId] ?? []).filter((c) => c.id !== id) } }),
  clear: (laneId) => {
    const byLane = { ...get().byLane };
    delete byLane[laneId];
    set({ byLane });
  },
  addImported: (laneId, list) => {
    const seen = get().importedIds[laneId] ?? [];
    const have = new Set(seen);
    const fresh = list.filter((c) => !have.has(c.id));
    let next = get().next;
    const added: ReviewComment[] = fresh.map((c) => ({
      id: next++,
      path: c.path,
      line: c.line,
      side: c.side === "old" ? "old" : "new",
      text: "",
      comment: c.body,
      imported: { id: c.id, author: c.author, fenced: c.fenced },
    }));
    set({
      next,
      byLane: { ...get().byLane, [laneId]: [...(get().byLane[laneId] ?? []), ...added] },
      importedIds: { ...get().importedIds, [laneId]: [...seen, ...fresh.map((c) => c.id)] },
    });
    return added.length;
  },
  addOpinion: (op) => {
    let id = get().next;
    const added: ReviewComment[] = op.comments.map((c) => ({
      path: c.path,
      side: "new",
      line: c.line,
      text: c.text,
      comment: c.comment,
      opinion: true,
      id: id++,
    }));
    if (op.general)
      added.push({
        path: "",
        side: "new",
        line: 0,
        text: "",
        comment: op.general,
        general: true,
        opinion: true,
        id: id++,
      });
    const list = get().byLane[op.laneId] ?? [];
    set({ next: id, byLane: { ...get().byLane, [op.laneId]: [...list, ...added] } });
  },
  reviewing: {},
  setReviewing: (laneId, r) => {
    const reviewing = { ...get().reviewing };
    if (r) reviewing[laneId] = r;
    else delete reviewing[laneId];
    set({ reviewing });
  },
}));

/** "Import review comments" (F-WT-08): the pull request's line comments
 * into the review bar, through the user's gh. */
export async function importReviewComments(
  laneId: string,
): Promise<{ added: number; outdated: number; cut: number } | { error: IpcError }> {
  try {
    // The ids imported before go along, so the cap counts only new ones.
    const seen = useReviews.getState().importedIds[laneId] ?? [];
    const got = await invoke<ImportedComments>("lane_review_comments", { laneId, seen });
    return {
      added: useReviews.getState().addImported(laneId, got.comments),
      outdated: got.outdated,
      cut: got.cut,
    };
  } catch (e) {
    return { error: e as IpcError };
  }
}

/** Opens a thread in Thread mode (the reviewer's, F-WT-05). */
export function openThread(threadId: string): void {
  switchMode("thread");
  useThreads.getState().select(threadId);
}

/** The agents running in a worktree's panes: where a review can go. */
export function worktreeAgents(items: Record<string, DashboardItem>, laneId: string): DashboardItem[] {
  return Object.values(items)
    .filter((i) => i.source.type === "pane" && i.workspaceId === laneId && i.engineId !== null)
    .sort((a, b) => a.key.localeCompare(b.key));
}

export function agentName(i: DashboardItem): string {
  return i.humanName ?? i.engineLabel ?? i.title ?? i.key;
}

type Editing = { target: DiffLineTarget; id: number | null; text: string } | null;

function CommentEditor({
  value,
  onChange,
  onSave,
  onCancel,
}: {
  value: string;
  onChange: (v: string) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="flex flex-col gap-2" data-testid="review-editor">
      <Textarea
        label={t("worktree.review.commentLabel")}
        hideLabel
        autoFocus
        minRows={2}
        maxRows={8}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            onSave();
          }
          if (e.key === "Escape") {
            e.stopPropagation();
            onCancel();
          }
        }}
        data-testid="review-comment-input"
      />
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onCancel}>
          {t("common.cancel")}
        </Button>
        <Button
          size="sm"
          variant="primary"
          disabled={value.trim().length === 0}
          onClick={onSave}
          data-testid="review-comment-save"
        >
          {t("worktree.review.saveComment")}
        </Button>
      </div>
    </div>
  );
}

/** The worktree's diff with line comments and the review bar (UI-152). */
export function ReviewDiff({
  wt,
  files,
  labels,
  onSecondOpinion,
}: {
  wt: WorkspaceSummary;
  files: FileDiff[];
  labels: Partial<DiffViewLabels>;
  /** F-WT-05: opens the reviewer sheet. */
  onSecondOpinion?: () => void;
}) {
  const comments = useReviews((s) => s.byLane[wt.id]) ?? [];
  const reviewing = useReviews((s) => s.reviewing[wt.id]) ?? null;
  const engineLabel = useEngines((s) => (reviewing ? s.label(reviewing.engineId) : null));
  const { add, edit, remove, clear } = useReviews.getState();
  const [editing, setEditing] = useState<Editing>(null);
  const items = useDashboard((s) => s.items);
  const agents = useMemo(() => worktreeAgents(items, wt.id), [items, wt.id]);
  const [target, setTarget] = useState<string>("");
  const [sending, setSending] = useState(false);
  const { show } = useToast();
  const branch = wt.laneBranch ?? wt.name;
  // F-CD-23: names on the new side, in the worktree; results open in Files.
  const names = useMemo(
    () => diffNames({ type: "workspace", workspaceId: wt.id }, () => useWorktrees.setState({ ask: null })),
    [wt.id],
  );
  const chosen = agents.find((a) => a.key === target) ?? agents[0] ?? null;
  // F-WT-08: once its repository is allowed and it has a pull request.
  const pr = useWorktrees((s) => (s.checks[wt.id]?.state === "ready" ? s.checks[wt.id]?.pr : null));
  const [importing, setImporting] = useState(false);
  const doImport = async () => {
    setImporting(true);
    const out = await importReviewComments(wt.id);
    setImporting(false);
    if ("error" in out)
      show({ tone: "error", title: t("worktree.review.importFailed", { reason: out.error.message }) });
    else {
      const left = [
        out.outdated > 0 ? t("worktree.review.importOutdated", { n: out.outdated }) : null,
        out.cut > 0 ? t("worktree.review.importCut", { n: out.cut }) : null,
      ].filter((x): x is string => x !== null);
      const body = left.length > 0 ? left.join(" ") : undefined;
      if (out.added === 0) show({ title: t("worktree.review.importedNone"), body });
      else show({ tone: "success", title: t("worktree.review.imported", { n: out.added }), body });
    }
  };

  const save = () => {
    if (!editing || editing.text.trim().length === 0) return;
    if (editing.id === null) add(wt.id, editing.target, editing.text.trim());
    else edit(wt.id, editing.id, editing.text.trim());
    setEditing(null);
  };

  // Several comments can share a line (an imported thread).
  const byLine: Record<string, React.ReactNode[]> = {};
  const general = comments.filter((c) => c.general);
  for (const c of comments) {
    if (editing?.id === c.id || c.general) continue;
    (byLine[diffLineKey(c)] ??= []).push(
      <div
        key={c.id}
        data-testid="review-comment"
        data-line={diffLineKey(c)}
        data-imported={c.imported ? "true" : undefined}
        className="flex items-start gap-2 text-sm"
      >
        <p className="min-w-0 flex-1 whitespace-pre-wrap text-fg">
          {c.imported ? (
            <span className="mr-1 font-medium text-fg-muted">
              {t("worktree.review.importedBy", { author: c.imported.author })}
            </span>
          ) : null}
          {c.comment}
        </p>
        {!c.imported && (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => setEditing({ target: c, id: c.id, text: c.comment })}
          >
            {t("worktree.review.edit")}
          </Button>
        )}
        <Button size="sm" variant="ghost" onClick={() => remove(wt.id, c.id)}>
          {t("worktree.review.remove")}
        </Button>
      </div>,
    );
  }
  const annotations: Record<string, React.ReactNode> = {};
  for (const [key, nodes] of Object.entries(byLine))
    annotations[key] = nodes.length === 1 ? nodes[0] : <div className="flex flex-col gap-1">{nodes}</div>;
  const editor = editing && (
    <CommentEditor
      value={editing.text}
      onChange={(text) => setEditing({ ...editing, text })}
      onSave={save}
      onCancel={() => setEditing(null)}
    />
  );
  if (editing && !general.some((c) => c.id === editing.id))
    annotations[diffLineKey(editing.target)] = (
      <CommentEditor
        value={editing.text}
        onChange={(text) => setEditing({ ...editing, text })}
        onSave={save}
        onCancel={() => setEditing(null)}
      />
    );

  const prompt = comments.length > 0 ? buildReviewPrompt(branch, comments) : "";
  const send = async () => {
    if (!chosen || chosen.source.type !== "pane") return;
    setSending(true);
    try {
      await invoke("pty_type_prompt", { paneId: chosen.source.paneId, text: prompt });
      clear(wt.id);
      show({ tone: "success", title: t("worktree.review.sent", { name: agentName(chosen) }) });
    } catch (e) {
      show({ tone: "error", title: t("worktree.review.sendFailed", { reason: (e as IpcError).message }) });
    } finally {
      setSending(false);
    }
  };
  const copy = () => {
    void navigator.clipboard?.writeText(prompt);
    show({ title: t("worktree.review.copied") });
  };

  return (
    <div className="flex min-h-0 flex-col gap-3">
      {pr ? (
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="secondary"
            loading={importing}
            onClick={() => void doImport()}
            data-testid="review-import"
          >
            {t("worktree.review.import", { n: pr.number })}
          </Button>
        </div>
      ) : null}
      {onSecondOpinion && (
        <div className="flex justify-end">
          <Button
            size="sm"
            variant="secondary"
            disabled={files.length === 0 || reviewing !== null}
            title={files.length === 0 ? t("worktree.opinion.noChanges") : undefined}
            onClick={onSecondOpinion}
            data-testid="review-second-opinion"
          >
            {t("worktree.opinion.menu")}
          </Button>
        </div>
      )}
      {general.map((c) => (
        <div
          key={c.id}
          data-testid="review-general"
          className="flex flex-col gap-1 rounded-md border border-border bg-surface-2 px-3 py-2 text-sm"
        >
          <span className="text-xs font-semibold text-fg-muted">{t("worktree.opinion.general")}</span>
          {editing?.id === c.id ? (
            editor
          ) : (
            <div className="flex items-start gap-2">
              <p className="min-w-0 flex-1 whitespace-pre-wrap text-fg">{c.comment}</p>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setEditing({ target: c, id: c.id, text: c.comment })}
              >
                {t("worktree.review.edit")}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => remove(wt.id, c.id)}>
                {t("worktree.review.remove")}
              </Button>
            </div>
          )}
        </div>
      ))}
      <DiffView
        files={files}
        labels={labels}
        names={names}
        comments={{
          label: (tg) =>
            t(tg.side === "new" ? "worktree.review.commentOn" : "worktree.review.commentOnRemoved", {
              path: tg.path,
              line: tg.line,
            }),
          onComment: (tg) => {
            // Imported comments aren't edited (the prompt sends GitHub's
            // fenced copy); a click on their line starts the user's own.
            const existing = comments.find(
              (c) => !c.imported && !c.general && diffLineKey(c) === diffLineKey(tg),
            );
            setEditing(
              existing
                ? { target: existing, id: existing.id, text: existing.comment }
                : { target: tg, id: null, text: "" },
            );
          },
          annotations,
        }}
      />
      {(comments.length > 0 || reviewing) && (
        <div
          data-testid="review-bar"
          className="sticky bottom-0 flex flex-wrap items-center gap-2 rounded-md border border-border bg-surface-2 px-3 py-2"
        >
          {reviewing && (
            <span className="flex items-center gap-2" data-testid="review-reviewing">
              <Spinner size={12} />
              <span className="text-sm text-fg-muted">
                {t("worktree.opinion.reviewing", { engine: engineLabel ?? reviewing.engineId })}
              </span>
              <Button size="sm" variant="ghost" onClick={() => openThread(reviewing.threadId)}>
                {t("worktree.opinion.openThread")}
              </Button>
            </span>
          )}
          {comments.length > 0 && (
            <span className="text-sm text-fg">{t("worktree.review.count", { n: comments.length })}</span>
          )}
          <span className="flex-1" />
          {agents.length > 1 && (
            <Select
              size="sm"
              ariaLabel={t("worktree.review.agent")}
              value={chosen?.key ?? ""}
              onValueChange={setTarget}
              options={agents.map((a) => ({ value: a.key, label: agentName(a) }))}
            />
          )}
          {comments.length > 0 && (
            <>
              <Button size="sm" variant="secondary" onClick={copy} data-testid="review-copy">
                {t("worktree.review.copy")}
              </Button>
              <Button
                size="sm"
                variant="primary"
                loading={sending}
                disabled={!chosen}
                title={chosen ? undefined : t("worktree.review.noAgent")}
                onClick={() => void send()}
                data-testid="review-send"
              >
                {chosen && agents.length === 1
                  ? t("worktree.review.sendTo", { name: agentName(chosen) })
                  : t("worktree.review.send")}
              </Button>
              {!chosen && (
                <p data-testid="review-no-agent" className="w-full text-xs text-fg-subtle">
                  {t("worktree.review.noAgent")}
                </p>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
