// UI-81: one task. Properties save as they change; the title saves on
// Enter or blur; the description edits with explicit Save / Cancel. While
// a field has unsaved text, another writer's change to other fields shows
// at once and a change to the same field raises the conflict banner
// (F-TK-03 merge rule, ./merge.ts).
import { useEffect, useMemo, useState } from "react";
import { ArrowLeft, MessageSquareReply, Play, X } from "lucide-react";
import type {
  Issue,
  IssueStatus,
  TaskComment,
  TaskDetail as Detail,
  TaskHistoryEntry,
} from "@tandem/protocol";
import {
  Avatar,
  Banner,
  Button,
  EmptyState,
  IconButton,
  Input,
  MarkdownView,
  Select,
  Skeleton,
  Tabs,
  Textarea,
  cx,
  useToast,
} from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { STATUS_ORDER, isIpcError, useTasks } from "../../stores/tasks";
import { age } from "../thread/groups";
import { ExecuteDialog, Runs } from "./Execute";
import { discard, draftOf, edit, keepMine, rebase, shown, type Draft, type EditField } from "./merge";

export const statusLabel = (s: IssueStatus) => t(`tasks.status.${s}` as MessageKey);
export const priorityLabel = (p: number) => t(`tasks.priority.${p}` as MessageKey);

/** An actor's name: the user, a teammate, or an agent session. */
export function actorName(id: string | null | undefined): string {
  if (!id) return t("tasks.unassigned");
  const actor = useTasks.getState().meta?.actors.find((a) => a.id === id);
  if (actor) return actor.kind === "human" ? t("tasks.you") : actor.name;
  return t("tasks.anAgent");
}

const FIELD_LABEL: Record<EditField, MessageKey> = {
  title: "tasks.field.title",
  description: "tasks.field.description",
};

function value(v: unknown): string {
  if (v === null || v === undefined || v === "") return t("tasks.history.none");
  if (Array.isArray(v)) return v.length ? v.join(", ") : t("tasks.history.none");
  if (typeof v === "string" && (STATUS_ORDER as string[]).includes(v)) return statusLabel(v as IssueStatus);
  if (typeof v === "object") return t("tasks.history.changed");
  return String(v);
}

function HistoryList({ entries }: { entries: TaskHistoryEntry[] }) {
  const now = Date.now();
  if (!entries.length) return <p className="px-1 py-2 text-sm text-fg-muted">{t("tasks.history.empty")}</p>;
  return (
    <ol className="flex flex-col gap-1.5 px-1 py-2" data-testid="task-history">
      {[...entries].reverse().map((e, i) => (
        <li key={i} className="flex items-baseline gap-2 text-sm">
          <span className="text-fg">{actorName(e.actor)}</span>
          <span className="min-w-0 flex-1 text-fg-muted">
            {e.field === "created"
              ? t("tasks.history.created")
              : e.field === "description" || e.field === "attempts"
                ? t("tasks.history.changedField", { field: e.field })
                : t("tasks.history.set", { field: e.field, from: value(e.from), to: value(e.to) })}
          </span>
          <span className="shrink-0 text-xs tabular-nums text-fg-subtle">{age(e.ts, now)}</span>
        </li>
      ))}
    </ol>
  );
}

function CommentItem({
  c,
  depth,
  onReply,
}: {
  c: TaskComment;
  depth: number;
  onReply: (c: TaskComment) => void;
}) {
  return (
    <li
      className={cx("flex flex-col gap-1 rounded-sm bg-surface-2 p-2", depth > 0 && "ml-6")}
      data-testid="task-comment"
    >
      <div className="flex items-center gap-2 text-xs text-fg-subtle">
        <Avatar size={20} id={c.actor} name={actorName(c.actor)} />
        <span className="text-sm text-fg">{actorName(c.actor)}</span>
        <span className="tabular-nums">{age(c.ts, Date.now())}</span>
        <span className="flex-1" />
        <IconButton
          size="sm"
          icon={<MessageSquareReply size={13} />}
          label={t("tasks.comments.reply")}
          onClick={() => onReply(c)}
        />
      </div>
      <MarkdownView markdown={c.body} />
    </li>
  );
}

function Comments({ comments }: { comments: TaskComment[] }) {
  const [body, setBody] = useState("");
  const [replyTo, setReplyTo] = useState<TaskComment | null>(null);
  const [busy, setBusy] = useState(false);
  const { show: toast } = useToast();
  // Threads: top-level comments in order, each followed by its replies.
  const ordered = useMemo(() => {
    const roots = comments.filter((c) => !c.parent_id || !comments.some((p) => p.id === c.parent_id));
    const out: Array<{ c: TaskComment; depth: number }> = [];
    const walk = (c: TaskComment, depth: number) => {
      out.push({ c, depth });
      comments.filter((r) => r.parent_id === c.id).forEach((r) => walk(r, Math.min(depth + 1, 3)));
    };
    roots.forEach((c) => walk(c, 0));
    return out;
  }, [comments]);
  const send = async () => {
    if (!body.trim() || busy) return;
    setBusy(true);
    const r = await useTasks.getState().comment(body, replyTo?.id);
    setBusy(false);
    if (isIpcError(r)) {
      toast({ tone: "error", title: r.message });
      return;
    }
    setBody("");
    setReplyTo(null);
  };
  return (
    <div className="flex flex-col gap-2 px-1 py-2">
      {ordered.length === 0 ? (
        <p className="text-sm text-fg-muted">{t("tasks.comments.empty")}</p>
      ) : (
        <ul className="flex flex-col gap-1.5" data-testid="task-comments">
          {ordered.map(({ c, depth }) => (
            <CommentItem key={c.id} c={c} depth={depth} onReply={setReplyTo} />
          ))}
        </ul>
      )}
      {replyTo && (
        <Banner tone="info" onDismiss={() => setReplyTo(null)} dismissLabel={t("tasks.comments.cancelReply")}>
          {t("tasks.comments.replyingTo", { name: actorName(replyTo.actor) })}
        </Banner>
      )}
      <Textarea
        label={t("tasks.comments.label")}
        hideLabel
        rows={3}
        value={body}
        placeholder={t("tasks.comments.placeholder")}
        onChange={(e) => setBody(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            void send();
          }
        }}
        data-testid="task-comment-input"
      />
      <div className="flex justify-end">
        <Button size="sm" variant="primary" disabled={!body.trim() || busy} onClick={() => void send()}>
          {t("tasks.comments.send")}
        </Button>
      </div>
    </div>
  );
}

/** The open task, with local drafts merged against the saved copy. */
function DetailBody({ detail, onClose }: { detail: Detail; onClose?: () => void }) {
  const [executing, setExecuting] = useState(false);
  const meta = useTasks((s) => s.meta);
  const save = useTasks((s) => s.save);
  const { show: toast } = useToast();
  const [draft, setDraft] = useState<Draft>(() => draftOf(detail.issue));
  const [editingDescription, setEditingDescription] = useState(false);
  const [tab, setTab] = useState("comments");
  const issue = draft.base;
  // A newer copy (an agent, another window) rebases the drafts.
  useEffect(() => {
    setDraft((d) => (d.base.id === detail.issue.id ? rebase(d, detail.issue) : draftOf(detail.issue)));
  }, [detail.issue]);

  /** Saves fields at the revision the drafts are based on. */
  const commit = async (fields: Partial<Issue>, after?: (d: Draft) => Draft) => {
    const r = await save({ id: issue.id, expected_revision: issue.revision, ...fields } as never);
    if (isIpcError(r)) {
      if (r.code === "revision_conflict") {
        // Someone wrote first: fetch, rebase, and let the user decide.
        await useTasks.getState().refreshDetail();
        toast({ tone: "warning", title: t("tasks.savedMeanwhile") });
      } else {
        toast({ tone: "error", title: r.message });
      }
      return;
    }
    setDraft((d) => {
      const next = rebase(d, r);
      return after ? after(next) : next;
    });
  };

  const saveField = (field: EditField) => {
    const text = draft.edits[field];
    if (text === undefined || field in draft.conflicts) return;
    if (field === "title" && !text.trim()) {
      setDraft((d) => discard(d, "title"));
      return;
    }
    void commit({ [field]: field === "title" ? text.trim() : text } as Partial<Issue>, (d) =>
      discard(d, field),
    );
  };

  const conflicts = Object.keys(draft.conflicts) as EditField[];
  const actors = meta?.actors ?? [];
  const labels = meta?.labels ?? [];
  const projects = meta?.projects ?? [];
  return (
    <section
      className="flex min-h-0 min-w-0 flex-1 flex-col"
      aria-label={t("tasks.detail", { id: issue.id })}
      data-testid="task-detail"
    >
      <div className="flex h-9 shrink-0 items-center gap-1 border-b border-divider px-1.5">
        {onClose && (
          <IconButton
            size="sm"
            icon={<ArrowLeft size={14} />}
            label={t("tasks.back")}
            onClick={onClose}
            data-testid="task-back"
          />
        )}
        <span className="font-mono text-xs text-fg-subtle">{issue.id}</span>
        <span className="flex-1" />
        <Button
          size="sm"
          variant="secondary"
          iconLeft={<Play size={13} />}
          onClick={() => setExecuting(true)}
          data-testid="task-execute"
        >
          {t("tasks.execute")}
        </Button>
        <ExecuteDialog issue={issue} open={executing} onOpenChange={setExecuting} />
      </div>
      {conflicts.map((field) => (
        <Banner
          key={field}
          tone="warning"
          action={{
            label: t("tasks.conflict.keepMine"),
            onClick: () => {
              setDraft((d) => keepMine(d, field));
              void commit({ [field]: draft.edits[field] } as Partial<Issue>, (d) => discard(d, field));
            },
          }}
        >
          <span data-testid="task-conflict">
            {t("tasks.conflict.body", { field: t(FIELD_LABEL[field]) })}{" "}
            <button
              type="button"
              className="underline underline-offset-2"
              onClick={() => {
                setDraft((d) => discard(d, field));
                if (field === "description") setEditingDescription(false);
              }}
            >
              {t("tasks.conflict.takeTheirs")}
            </button>
          </span>
        </Banner>
      ))}
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-auto p-3">
        <Input
          label={t("tasks.field.title")}
          hideLabel
          value={shown(draft, "title")}
          onChange={(e) => setDraft((d) => edit(d, "title", e.target.value))}
          onBlur={() => saveField("title")}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              saveField("title");
            } else if (e.key === "Escape" && draft.edits.title !== undefined) {
              e.stopPropagation();
              setDraft((d) => discard(d, "title"));
            }
          }}
          className="text-md font-semibold"
          data-testid="task-title"
        />
        <div className="grid grid-cols-2 gap-2">
          <Select
            label={t("tasks.field.status")}
            size="sm"
            value={issue.status}
            options={STATUS_ORDER.map((s) => ({ value: s, label: statusLabel(s) }))}
            onValueChange={(v) => void commit({ status: v as IssueStatus })}
          />
          <Select
            label={t("tasks.field.priority")}
            size="sm"
            value={String(issue.priority)}
            options={[0, 1, 2, 3, 4].map((p) => ({ value: String(p), label: priorityLabel(p) }))}
            onValueChange={(v) => void commit({ priority: Number(v) })}
          />
          <Select
            label={t("tasks.field.assignee")}
            size="sm"
            value={issue.assignee ?? ""}
            options={[
              { value: "", label: t("tasks.unassigned") },
              ...actors.map((a) => ({ value: a.id, label: a.kind === "human" ? t("tasks.you") : a.name })),
            ]}
            onValueChange={(v) => void commit({ assignee: v || null })}
          />
          <Select
            label={t("tasks.field.project")}
            size="sm"
            value={issue.project ?? ""}
            options={[
              { value: "", label: t("tasks.noProject") },
              ...projects.map((p) => ({ value: p.id, label: p.name })),
            ]}
            onValueChange={(v) => void commit({ project: v || null })}
          />
        </div>
        {labels.length > 0 && (
          <div className="flex flex-wrap gap-1" role="group" aria-label={t("tasks.field.labels")}>
            {labels.map((l) => {
              const on = issue.labels.includes(l.id);
              return (
                <button
                  key={l.id}
                  type="button"
                  aria-pressed={on}
                  onClick={() =>
                    void commit({
                      labels: on ? issue.labels.filter((x) => x !== l.id) : [...issue.labels, l.id],
                    })
                  }
                  className={cx(
                    "rounded-full border px-2 py-0.5 text-xs focus-visible:shadow-focus focus-visible:outline-none",
                    on ? "border-accent bg-accent-soft text-fg" : "border-border text-fg-muted hover:text-fg",
                  )}
                >
                  {l.name}
                </button>
              );
            })}
          </div>
        )}
        <div className="flex flex-col gap-1.5">
          <div className="flex items-center justify-between">
            <h3 className="text-xs font-medium uppercase tracking-caps text-fg-subtle">
              {t("tasks.field.description")}
            </h3>
            {!editingDescription && (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setEditingDescription(true)}
                data-testid="task-description-edit"
              >
                {t("tasks.description.edit")}
              </Button>
            )}
          </div>
          {editingDescription ? (
            <>
              <Textarea
                label={t("tasks.field.description")}
                hideLabel
                rows={8}
                value={shown(draft, "description")}
                onChange={(e) => setDraft((d) => edit(d, "description", e.target.value))}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                    e.preventDefault();
                    saveField("description");
                    setEditingDescription(false);
                  }
                }}
                data-testid="task-description-input"
              />
              <div className="flex justify-end gap-2">
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => {
                    setDraft((d) => discard(d, "description"));
                    setEditingDescription(false);
                  }}
                >
                  {t("tasks.description.cancel")}
                </Button>
                <Button
                  size="sm"
                  variant="primary"
                  disabled={"description" in draft.conflicts}
                  onClick={() => {
                    saveField("description");
                    setEditingDescription(false);
                  }}
                  data-testid="task-description-save"
                >
                  {t("tasks.description.save")}
                </Button>
              </div>
            </>
          ) : issue.description.trim() ? (
            <MarkdownView markdown={issue.description} />
          ) : (
            <p className="text-sm text-fg-subtle">{t("tasks.description.empty")}</p>
          )}
        </div>
        <Runs issueId={issue.id} />
        {issue.result && (
          <div className="flex flex-col gap-1.5" data-testid="task-result">
            <h3 className="text-xs font-medium uppercase tracking-caps text-fg-subtle">
              {t("tasks.result")}
            </h3>
            <MarkdownView markdown={issue.result} />
          </div>
        )}
        <Tabs
          label={t("tasks.activity")}
          value={tab}
          onValueChange={setTab}
          items={[
            { id: "comments", label: t("tasks.comments.tab"), count: detail.comments.length },
            { id: "history", label: t("tasks.history.tab") },
          ]}
          renderPanel={(id) =>
            id === "comments" ? (
              <Comments comments={detail.comments} />
            ) : (
              <HistoryList entries={detail.history} />
            )
          }
        />
      </div>
    </section>
  );
}

export function TaskDetailView({ onClose }: { onClose?: () => void }) {
  const detail = useTasks((s) => s.detail);
  const error = useTasks((s) => s.detailError);
  const selectedId = useTasks((s) => s.selectedId);
  if (!selectedId) return null;
  if (error && !detail) {
    return (
      <EmptyState
        icon={<X size={20} />}
        title={t("tasks.detailError")}
        body={error}
        primary={
          <Button size="sm" onClick={() => void useTasks.getState().refreshDetail()}>
            {t("tasks.retry")}
          </Button>
        }
      />
    );
  }
  if (!detail || detail.issue.id !== selectedId) {
    return (
      <div className="flex flex-col gap-2 p-3" aria-busy="true">
        <Skeleton width="66%" height={24} />
        <Skeleton width="100%" height={64} />
      </div>
    );
  }
  return <DetailBody key={detail.issue.id} detail={detail} onClose={onClose} />;
}
