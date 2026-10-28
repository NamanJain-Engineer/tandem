import { memo, useEffect, useState, type ReactNode } from "react";
import { ArrowDown, CheckCircle2, ChevronDown, ChevronRight, Copy, Info, Paperclip, Pencil } from "lucide-react";
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
  cx,
  type DiffFile,
} from "@tandem/ui";
import { t } from "../../i18n";
import { MailRowView } from "./MailRow";
import { PluginApprovalFields, pluginApprovalLabels } from "../plugins/PluginApproval";
import { parseApproval } from "../plugins/model";
import { groupStatus, type Row, type ToolEvent } from "./rows";
import { diffCounts, statusLabel, toolView } from "./tool";
import { VirtualList } from "./VirtualList";

export type TranscriptActions = {
  approve: (id: string, scope: "once" | "thread" | "conversation") => void;
  deny: (id: string, reason: string | null) => void;
  retry: () => void;
  openTerminal: () => void;
  signIn: () => void;
  editResend: (text: string) => void;
  openLink: (href: string) => void;
  copy: (text: string) => void;
  /** F-TH-10: Act, then send this plan version. */
  implementPlan: (version: number) => void;
  /** Pre-fills the composer to ask for a revised plan. */
  revisePlan: () => void;
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
      <ToolOutput
        key="out"
        text={tool.output}
        note={tool.outputTruncated ? t("thread.tool.truncated") : undefined}
      />,
    );
  }
  if (tool.detail && tool.detail !== tool.argSummary && !tool.diff) {
    parts.push(
      <CodeBlock
        key="detail"
        code={tool.detail}
        copyLabel={t("thread.copy")}
        copiedLabel={t("thread.copied")}
      />,
    );
  }
  return parts.length ? <>{parts}</> : null;
}

function Tool({ tool }: { tool: ToolEvent }) {
  const v = toolView(tool);
  const body = <ToolBody tool={tool} />;
  const has = !!(tool.diff || tool.output || (tool.detail && tool.detail !== tool.argSummary));
  return (
    <ToolCallBlock
      icon={v.icon}
      verb={v.verb}
      arg={v.arg}
      meta={v.meta}
      status={tool.status}
      statusLabel={v.statusLabel}
    >
      {has ? body : undefined}
    </ToolCallBlock>
  );
}

function Reasoning({
  text,
  durationMs,
  streaming,
}: {
  text: string;
  durationMs: number | null;
  streaming: boolean;
}) {
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
      {open && (
        <p className="whitespace-pre-wrap border-l border-divider pl-3 text-sm text-fg-subtle">{text}</p>
      )}
    </div>
  );
}

function decisionText(d: ApprovalDecisionView): { tone: "allowed" | "denied"; text: string } {
  if (d.type === "allow")
    return {
      tone: "allowed",
      text:
        d.scope === "once"
          ? t("thread.approval.allowedOnce")
          : d.scope === "conversation"
            ? t("plugins.approval.allowedChat")
            : t("thread.approval.allowedThread"),
    };
  if (d.type === "deny")
    return {
      tone: "denied",
      text: d.reason ? t("thread.approval.deniedWhy", { reason: d.reason }) : t("thread.approval.denied"),
    };
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
      <span
        aria-hidden="true"
        className="size-2 animate-pulse rounded-full bg-working motion-reduce:animate-none"
      />
      {t("thread.working", { s })}
    </p>
  );
}

function Hover({
  children,
  actions,
  className,
}: {
  children: ReactNode;
  actions: ReactNode;
  className?: string;
}) {
  return (
    <div className={cx("group relative", className)}>
      {children}
      <div className="absolute -top-3 right-0 hidden gap-0.5 rounded-sm border border-border bg-surface-3 p-0.5 shadow-1 group-focus-within:flex group-hover:flex">
        {actions}
      </div>
    </div>
  );
}

/** A plan version (UI_SPEC §4.3): its steps, and Implement / Revise / Copy. */
export function PlanCard({
  version,
  text,
  actions,
}: {
  version: number;
  text: string | null;
  actions: TranscriptActions;
}) {
  return (
    <Callout
      tone="accent"
      title={t("thread.plan", { n: version })}
      className="border-l-4"
      actions={
        text !== null && (
          <>
            <Button
              size="sm"
              variant="primary"
              onClick={() => actions.implementPlan(version)}
              data-testid="plan-implement"
            >
              {t("thread.plan.implement")}
            </Button>
            <Button size="sm" variant="secondary" onClick={actions.revisePlan}>
              {t("thread.plan.revise")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              iconLeft={<Copy size={14} />}
              onClick={() => actions.copy(text)}
            >
              {t("thread.copy")}
            </Button>
          </>
        )
      }
    >
      {text !== null ? <MarkdownView markdown={text} onLink={actions.openLink} /> : t("thread.plan.missing")}
    </Callout>
  );
}

const RowView = memo(function RowView({
  row,
  actions,
  approvalLabel,
  startedAt,
  planText,
}: {
  row: Row;
  actions: TranscriptActions;
  approvalLabel: (kind: string) => string;
  startedAt: (turnId: string) => number | null;
  planText: (version: number) => string | null;
}) {
  switch (row.kind) {
    case "user":
      return (
        <div className="flex justify-end">
          <Hover
            // The cap sits on the wrapper: a percentage of a shrink-to-fit
            // box would squeeze short messages to a letter per line.
            className="max-w-[72%]"
            actions={
              <>
                <IconButton
                  size="sm"
                  icon={<Pencil size={14} />}
                  label={t("thread.editResend")}
                  onClick={() => actions.editResend(row.text)}
                />
                <IconButton
                  size="sm"
                  icon={<Copy size={14} />}
                  label={t("thread.copy")}
                  onClick={() => actions.copy(row.text)}
                />
              </>
            }
          >
            <div
              data-testid="thread-user"
              className="flex flex-col gap-1 rounded-md bg-surface-3 px-3 py-2 text-md text-fg"
            >
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
    case "tools": {
      // The latest call stays in view; the ones before it fold (D-2604).
      const earlier = row.tools.slice(0, -1);
      const last = row.tools[row.tools.length - 1];
      return (
        <ToolGroup
          summary={t("thread.tools.previous", { n: earlier.length })}
          collapseLabel={t("thread.tools.collapse")}
          status={groupStatus(earlier)}
          statusLabel={statusLabel(groupStatus(earlier))}
          tail={last && <Tool tool={last} />}
        >
          {earlier.map((tool) => (
            <Tool key={tool.id} tool={tool} />
          ))}
        </ToolGroup>
      );
    }
    case "summary":
      return (
        <p
          data-testid="turn-summary"
          className="flex flex-wrap items-center gap-x-2 gap-y-0.5 px-1 py-0.5 text-sm text-fg-muted"
        >
          <span className="sr-only">{t("thread.summary.labelPrefix")}</span>
          <CheckCircle2 size={13} aria-hidden="true" className="shrink-0 text-done-text" />
          {row.files > 0 && (
            <span>
              {t("thread.summary.files", { n: row.files })}{" "}
              <span className="tabular-nums text-diff-add-fg">{t("thread.summary.add", { n: row.add })}</span>{" "}
              <span className="tabular-nums text-diff-del-fg">{t("thread.summary.del", { n: row.del })}</span>
            </span>
          )}
          {row.files > 0 && row.tests && <span aria-hidden="true">·</span>}
          {row.tests && (
            <span className={cx("tabular-nums", row.tests.failed > 0 && "text-danger")}>
              {t("thread.tool.tests", { passed: row.tests.passed, failed: row.tests.failed })}
            </span>
          )}
        </p>
      );
    case "assistant":
      return (
        <Hover
          actions={
            <IconButton
              size="sm"
              icon={<Copy size={14} />}
              label={t("thread.copyMarkdown")}
              onClick={() => actions.copy(row.markdown)}
            />
          }
        >
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
        return (
          <ApprovalCard
            kind={r.kind}
            title={r.summary}
            onAllow={() => undefined}
            onDeny={() => undefined}
            resolved={decisionText(row.decision)}
          />
        );
      }
      const plugin = parseApproval(r.detail);
      if (plugin) {
        return (
          <ApprovalCard
            kind={r.kind}
            title={t("plugins.approval.title", { plugin: plugin.pluginLabel })}
            detail={<PluginApprovalFields detail={plugin} />}
            risk={plugin.riskClass === "spend" ? "high" : "low"}
            riskText={plugin.cost ? t("plugins.approval.spendNote", { cost: plugin.cost }) : undefined}
            allowThread={r.options.includes("conversation")}
            onAllow={() => actions.approve(r.id, "once")}
            onAllowThread={() => actions.approve(r.id, "conversation")}
            onDeny={(reason) => actions.deny(r.id, reason)}
            labels={{ ...approvalLabels(r.kind), ...pluginApprovalLabels() }}
          />
        );
      }
      return (
        <ApprovalCard
          kind={r.kind}
          title={approvalLabel(r.kind)}
          detail={
            <CodeBlock
              code={r.detail || r.summary}
              copyLabel={t("thread.copy")}
              copiedLabel={t("thread.copied")}
            />
          }
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
        <p
          className="flex items-center justify-center gap-1.5 py-1 text-sm text-fg-subtle"
          data-testid="thread-end"
        >
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
          {row.limit.resetsAt
            ? ` ${t("thread.limit.resets", { at: new Date(row.limit.resetsAt).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) })}`
            : ""}
        </Callout>
      );
    case "mail":
      return <MailRowView mailId={row.mailId} from={row.from} to={row.to} preview={row.preview} />;
    case "divider":
      return (
        <div className="flex items-center gap-3 py-1 text-sm text-fg-subtle">
          <span className="h-px flex-1 bg-divider" />
          {row.text}
          <span className="h-px flex-1 bg-divider" />
        </div>
      );
    case "plan":
      return (
        <div data-testid="thread-plan" data-version={row.version}>
          <PlanCard version={row.version} text={planText(row.version)} actions={actions} />
        </div>
      );
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
    steadying: t("approval.waiting"),
  };
}
export { approvalLabels };

/** UI-21: the thread's transcript, virtualized. */
export function Transcript({
  rows,
  actions,
  approvalLabel,
  startedAt,
  planText,
  onNearTop,
}: {
  rows: Row[];
  actions: TranscriptActions;
  approvalLabel: (kind: string) => string;
  startedAt: (turnId: string) => number | null;
  planText: (version: number) => string | null;
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
      render={(row) => (
        <RowView
          row={row}
          actions={actions}
          approvalLabel={approvalLabel}
          startedAt={startedAt}
          planText={planText}
        />
      )}
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
