import { memo, useEffect, useState, type ReactNode } from "react";
import { ArrowDown, ChevronDown, ChevronRight, Copy, Info, Paperclip, Pencil } from "lucide-react";
import type { ApprovalDecisionView } from "@tandem/protocol";
import {
  ApprovalCard,
  Banner,
  Button,
  Callout,
  CodeBlock,
  DiffView,
  IconButton,
  MarkdownView,
  ToolCallBlock,
  ToolGroup,
  ToolOutput,
  type DiffFile,
} from "@tandem/ui";
import { t } from "../../i18n";
import { groupStatus, type Row, type ToolEvent } from "./rows";
import { diffCounts, statusLabel, toolView } from "./tool";
import { VirtualList } from "./VirtualList";

export type TranscriptActions = {
  approve: (id: string, scope: "once" | "thread") => void;
  deny: (id: string, reason: string | null) => void;
  retry: () => void;
  openTerminal: () => void;
  signIn: () => void;
  editResend: (text: string) => void;
  openLink: (href: string) => void;
  copy: (text: string) => void;
};

/** A unified diff (with `--- a/` / `+++ b/` headers per file) as DiffView files. */
export function diffFiles(diff: string): DiffFile[] {
  const files: DiffFile[] = [];
  let cur: { path: string; lines: string[] } | null = null;
  const lines = diff.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] ?? "";
    const next = lines[i + 1] ?? "";
    if (line.startsWith("--- ") && next.startsWith("+++ ")) {
      if (cur) files.push(toFile(cur));
      const path = next.slice(4).replace(/^b\//, "").trim();
      cur = { path, lines: [] };
      i++;
      continue;
    }
    if (!cur) cur = { path: "", lines: [] };
    cur.lines.push(line);
  }
  if (cur) files.push(toFile(cur));
  return files.filter((f) => f.patch.trim());
}

function toFile({ path, lines }: { path: string; lines: string[] }): DiffFile {
  // A body without hunk headers still renders as one hunk.
  const body = lines.some((l) => l.startsWith("@@")) ? lines : ["@@ -1 +1 @@", ...lines];
  const patch = body.join("\n");
  const { add, del } = diffCounts(patch);
  return { path, change: "modified", additions: add, deletions: del, binary: false, patch, truncated: false };
}

function ToolBody({ tool }: { tool: ToolEvent }) {
  const parts: ReactNode[] = [];
  if (tool.diff) parts.push(<DiffView key="diff" files={diffFiles(tool.diff)} />);
  if (tool.output) {
    parts.push(
      <ToolOutput key="out" text={tool.output} note={tool.outputTruncated ? t("thread.tool.truncated") : undefined} />,
    );
  }
  if (tool.detail && tool.detail !== tool.argSummary && !tool.diff) {
    parts.push(<CodeBlock key="detail" code={tool.detail} copyLabel={t("thread.copy")} copiedLabel={t("thread.copied")} />);
  }
  return parts.length ? <>{parts}</> : null;
}

function Tool({ tool }: { tool: ToolEvent }) {
  const v = toolView(tool);
  const body = <ToolBody tool={tool} />;
  const has = !!(tool.diff || tool.output || (tool.detail && tool.detail !== tool.argSummary));
  return (
    <ToolCallBlock icon={v.icon} verb={v.verb} arg={v.arg} meta={v.meta} status={tool.status} statusLabel={v.statusLabel}>
      {has ? body : undefined}
    </ToolCallBlock>
  );
}

function Reasoning({ text, durationMs, streaming }: { text: string; durationMs: number | null; streaming: boolean }) {
  const [open, setOpen] = useState(false);
  const label = streaming
    ? t("thread.reasoning.active")
    : durationMs !== null
      ? t("thread.reasoning.done", { n: Math.max(1, Math.round(durationMs / 1000)) })
      : t("thread.reasoning.doneShort");
  return (
    <div className="flex flex-col gap-1">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="inline-flex items-center gap-1 self-start rounded-sm px-1 text-sm text-fg-subtle hover:text-fg-muted focus-visible:shadow-focus focus-visible:outline-none"
      >
        {open ? <ChevronDown size={12} aria-hidden="true" /> : <ChevronRight size={12} aria-hidden="true" />}
        {label}
      </button>
      {open && <p className="whitespace-pre-wrap border-l border-divider pl-3 text-sm text-fg-subtle">{text}</p>}
    </div>
  );
}

function decisionText(d: ApprovalDecisionView): { tone: "allowed" | "denied"; text: string } {
  if (d.type === "allow") return { tone: "allowed", text: d.scope === "once" ? t("thread.approval.allowedOnce") : t("thread.approval.allowedThread") };
  if (d.type === "deny") return { tone: "denied", text: d.reason ? t("thread.approval.deniedWhy", { reason: d.reason }) : t("thread.approval.denied") };
  return { tone: "denied", text: t("thread.approval.expired", { n: 30 }) };
}

function Working({ startedAt }: { startedAt: number | null }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, []);
  const s = startedAt ? Math.max(0, Math.round((now - startedAt) / 1000)) : 0;
  return (
    <p data-testid="thread-working" className="flex items-center gap-2 px-1 text-sm text-fg-muted">
      <span aria-hidden="true" className="size-2 animate-pulse rounded-full bg-working motion-reduce:animate-none" />
      {t("thread.working", { s })}
    </p>
  );
}

function Hover({ children, actions }: { children: ReactNode; actions: ReactNode }) {
  return (
    <div className="group relative">
      {children}
      <div className="absolute -top-3 right-0 hidden gap-0.5 rounded-sm border border-border bg-surface-3 p-0.5 shadow-1 group-focus-within:flex group-hover:flex">
        {actions}
      </div>
    </div>
  );
}

const RowView = memo(function RowView({
  row,
  actions,
  approvalLabel,
  startedAt,
}: {
  row: Row;
  actions: TranscriptActions;
  approvalLabel: (kind: string) => string;
  startedAt: (turnId: string) => number | null;
}) {
  switch (row.kind) {
    case "user":
      return (
        <div className="flex justify-end">
          <Hover
            actions={
              <>
                <IconButton size="sm" icon={<Pencil size={14} />} label={t("thread.editResend")} onClick={() => actions.editResend(row.text)} />
                <IconButton size="sm" icon={<Copy size={14} />} label={t("thread.copy")} onClick={() => actions.copy(row.text)} />
              </>
            }
          >
            <div data-testid="thread-user" className="flex max-w-[72%] flex-col gap-1 rounded-md bg-surface-3 px-3 py-2 text-md text-fg">
              {row.attachments.length > 0 && (
                <span className="inline-flex items-center gap-1 text-sm text-fg-muted">
                  <Paperclip size={12} aria-hidden="true" />
                  {t("thread.attachments", { n: row.attachments.length })}
                </span>
              )}
              <p className="whitespace-pre-wrap break-words">{row.text}</p>
            </div>
          </Hover>
        </div>
      );
    case "reasoning":
      return <Reasoning text={row.text} durationMs={row.durationMs} streaming={row.streaming} />;
    case "tool":
      return <Tool tool={row.tool} />;
    case "tools":
      return (
        <ToolGroup
          summary={t("thread.tools", { n: row.tools.length })}
          collapseLabel={t("thread.tools.collapse")}
          status={groupStatus(row.tools)}
          statusLabel={statusLabel(groupStatus(row.tools))}
        >
          {row.tools.map((tool) => (
            <Tool key={tool.id} tool={tool} />
          ))}
        </ToolGroup>
      );
    case "assistant":
      return (
        <Hover actions={<IconButton size="sm" icon={<Copy size={14} />} label={t("thread.copyMarkdown")} onClick={() => actions.copy(row.markdown)} />}>
          <div data-testid="thread-assistant">
            <MarkdownView
              markdown={row.markdown}
              streaming={row.streaming}
              onLink={actions.openLink}
              copyLabel={t("thread.copy")}
              copiedLabel={t("thread.copied")}
              doneLabel={t("thread.taskDone")}
              todoLabel={t("thread.taskTodo")}
              showAllLabel={(size) => t("thread.showAll", { size })}
            />
          </div>
        </Hover>
      );
    case "approval": {
      const r = row.request;
      if (row.decision) {
        return <ApprovalCard kind={r.kind} title={r.summary} onAllow={() => undefined} onDeny={() => undefined} resolved={decisionText(row.decision)} />;
      }
      return (
        <ApprovalCard
          kind={r.kind}
          title={approvalLabel(r.kind)}
          detail={<CodeBlock code={r.detail || r.summary} copyLabel={t("thread.copy")} copiedLabel={t("thread.copied")} />}
          context={r.summary}
          allowThread={r.options.includes("thread")}
          onAllow={() => actions.approve(r.id, "once")}
          onAllowThread={() => actions.approve(r.id, "thread")}
          onDeny={(reason) => actions.deny(r.id, reason)}
          labels={approvalLabels(r.kind)}
        />
      );
    }
    case "note":
      return (
        <p className="flex items-center justify-center gap-1.5 py-1 text-sm text-fg-subtle">
          <Info size={12} aria-hidden="true" />
          {row.text}
        </p>
      );
    case "end":
      return (
        <p className="flex items-center justify-center gap-1.5 py-1 text-sm text-fg-subtle" data-testid="thread-end">
          <Info size={12} aria-hidden="true" />
          {row.reason === "cancelled" ? t("thread.stopped") : t("thread.blocked", { n: row.blocked })}
        </p>
      );
    case "error": {
      const e = row.error;
      const action = !row.last
        ? undefined
        : e.action === "retry"
          ? { label: t("thread.retry"), onClick: actions.retry }
          : e.action === "open_terminal"
            ? { label: t("thread.openTerminal"), onClick: actions.openTerminal }
            : e.action === "sign_in"
              ? { label: t("thread.signIn"), onClick: actions.signIn }
              : undefined;
      return (
        <Banner tone="error" action={action} className="rounded-md">
          <span className="flex flex-col">
            <span>{e.message}</span>
            {e.hint && <span className="text-fg-muted">{e.hint}</span>}
          </span>
        </Banner>
      );
    }
    case "limit":
      return (
        <Callout tone="warning" title={t("thread.limit.title")}>
          {row.limit.message}
          {row.limit.resetsAt ? ` ${t("thread.limit.resets", { at: new Date(row.limit.resetsAt).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) })}` : ""}
        </Callout>
      );
    case "divider":
      return (
        <div className="flex items-center gap-3 py-1 text-sm text-fg-subtle">
          <span className="h-px flex-1 bg-divider" />
          {row.text}
          <span className="h-px flex-1 bg-divider" />
        </div>
      );
    case "plan":
      return <Callout tone="accent" title={t("thread.plan", { n: row.version })} />;
    case "working":
      return <Working startedAt={startedAt(row.turnId)} />;
  }
});

function approvalLabels(kind: string) {
  const file = kind === "file_write" || kind === "file_delete";
  return {
    waiting: t("thread.approval.waiting"),
    allow: file ? t("thread.approval.allow") : t("thread.approval.allowOnce"),
    allowThread: t("thread.approval.allowThread"),
    deny: t("thread.approval.deny"),
    denyExplain: t("thread.approval.denyExplain"),
    reason: t("thread.approval.reason"),
    sendReason: t("thread.approval.deny"),
    cancel: t("thread.approval.cancel"),
    showDetail: file ? t("thread.approval.viewDiff") : t("thread.approval.showDetail"),
    hideDetail: file ? t("thread.approval.hideDiff") : t("thread.approval.hideDetail"),
  };
}
export { approvalLabels };

/** UI-21: the thread's transcript, virtualized. */
export function Transcript({
  rows,
  actions,
  approvalLabel,
  startedAt,
  onNearTop,
}: {
  rows: Row[];
  actions: TranscriptActions;
  approvalLabel: (kind: string) => string;
  startedAt: (turnId: string) => number | null;
  onNearTop?: () => void;
}) {
  return (
    <VirtualList
      items={rows}
      getKey={rowKey}
      estimate={40}
      ariaLabel={t("thread.transcript")}
      onNearTop={onNearTop}
      className="h-full overflow-y-auto px-4 py-3 focus-visible:outline-none [&>div]:mx-auto [&>div]:max-w-3xl [&>div]:py-1.5"
      render={(row) => <RowView row={row} actions={actions} approvalLabel={approvalLabel} startedAt={startedAt} />}
      newActivity={(jump) => (
        <div className="absolute inset-x-0 bottom-3 flex justify-center">
          <Button size="sm" variant="secondary" iconLeft={<ArrowDown size={14} />} onClick={jump}>
            {t("thread.newActivity")}
          </Button>
        </div>
      )}
    />
  );
}

const rowKey = (r: Row) => r.key;
