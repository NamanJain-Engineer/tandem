import { useMemo, useState } from "react";
import { create } from "zustand";
import type { DashboardItem, FileDiff, IpcError, WorkspaceSummary } from "@tandem/protocol";
import {
  Button,
  DiffView,
  Select,
  Textarea,
  diffLineKey,
  useToast,
  type DiffLineTarget,
  type DiffViewLabels,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useDashboard } from "../../stores/dashboard";

/** One review comment on a worktree's diff (F-WT-02). */
export type ReviewComment = DiffLineTarget & { id: number; comment: string };

const QUOTE_MAX = 200;

/** Path order, then the line, new side before old on the same number. */
function byPlace(a: ReviewComment, b: ReviewComment): number {
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
 * prompts to engines. */
export function buildReviewPrompt(branch: string, comments: ReviewComment[]): string {
  const sorted = [...comments].sort(byPlace);
  const n = sorted.length;
  const head = `Review of your changes on ${branch} (${n} ${n === 1 ? "comment" : "comments"}):`;
  const items = sorted.map((c, i) => {
    const where = c.side === "new" ? `${c.path}:${c.line}` : `${c.path} (removed line ${c.line})`;
    const body = c.comment
      .trim()
      .split("\n")
      .map((l) => `   ${l}`)
      .join("\n");
    const q = quote(c.text);
    return `${i + 1}. ${where}\n${q ? `   > ${q}\n` : ""}${body}`;
  });
  return `${head}\n\n${items.join("\n\n")}\n\nAddress each comment, then say what you changed.`;
}

type ReviewsState = {
  /** Comments per worktree, kept until sent or the app quits (D-2205). */
  byLane: Record<string, ReviewComment[]>;
  next: number;
  add: (laneId: string, target: DiffLineTarget, comment: string) => void;
  edit: (laneId: string, id: number, comment: string) => void;
  remove: (laneId: string, id: number) => void;
  clear: (laneId: string) => void;
};

export const useReviews = create<ReviewsState>((set, get) => ({
  byLane: {},
  next: 1,
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
}));

/** The agents running in a worktree's panes: where a review can go. */
export function worktreeAgents(items: Record<string, DashboardItem>, laneId: string): DashboardItem[] {
  return Object.values(items)
    .filter((i) => i.source.type === "pane" && i.workspaceId === laneId && i.engineId !== null)
    .sort((a, b) => a.key.localeCompare(b.key));
}

function agentName(i: DashboardItem): string {
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
}: {
  wt: WorkspaceSummary;
  files: FileDiff[];
  labels: Partial<DiffViewLabels>;
}) {
  const comments = useReviews((s) => s.byLane[wt.id]) ?? [];
  const { add, edit, remove, clear } = useReviews.getState();
  const [editing, setEditing] = useState<Editing>(null);
  const items = useDashboard((s) => s.items);
  const agents = useMemo(() => worktreeAgents(items, wt.id), [items, wt.id]);
  const [target, setTarget] = useState<string>("");
  const [sending, setSending] = useState(false);
  const { show } = useToast();
  const branch = wt.laneBranch ?? wt.name;
  const chosen = agents.find((a) => a.key === target) ?? agents[0] ?? null;

  const save = () => {
    if (!editing || editing.text.trim().length === 0) return;
    if (editing.id === null) add(wt.id, editing.target, editing.text.trim());
    else edit(wt.id, editing.id, editing.text.trim());
    setEditing(null);
  };

  const annotations: Record<string, React.ReactNode> = {};
  for (const c of comments) {
    if (editing?.id === c.id) continue;
    annotations[diffLineKey(c)] = (
      <div data-testid="review-comment" data-line={diffLineKey(c)} className="flex items-start gap-2 text-sm">
        <p className="min-w-0 flex-1 whitespace-pre-wrap text-fg">{c.comment}</p>
        <Button size="sm" variant="ghost" onClick={() => setEditing({ target: c, id: c.id, text: c.comment })}>
          {t("worktree.review.edit")}
        </Button>
        <Button size="sm" variant="ghost" onClick={() => remove(wt.id, c.id)}>
          {t("worktree.review.remove")}
        </Button>
      </div>
    );
  }
  if (editing)
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
      <DiffView
        files={files}
        labels={labels}
        comments={{
          label: (tg) =>
            t(tg.side === "new" ? "worktree.review.commentOn" : "worktree.review.commentOnRemoved", {
              path: tg.path,
              line: tg.line,
            }),
          onComment: (tg) => {
            const existing = comments.find((c) => diffLineKey(c) === diffLineKey(tg));
            setEditing(
              existing ? { target: existing, id: existing.id, text: existing.comment } : { target: tg, id: null, text: "" },
            );
          },
          annotations,
        }}
      />
      {comments.length > 0 && (
        <div
          data-testid="review-bar"
          className="sticky bottom-0 flex flex-wrap items-center gap-2 rounded-md border border-border bg-surface-2 px-3 py-2"
        >
          <span className="text-sm text-fg">{t("worktree.review.count", { n: comments.length })}</span>
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
        </div>
      )}
    </div>
  );
}
