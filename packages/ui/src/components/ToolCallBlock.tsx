import { useId, useState, type ReactNode } from "react";
import {
  Ban,
  Bot,
  Check,
  ChevronDown,
  ChevronRight,
  FileText,
  Globe,
  Pencil,
  Plug,
  Search,
  SquareTerminal,
  TriangleAlert,
  Wrench,
} from "lucide-react";
import { cx } from "../cx";
import { Spinner } from "./Spinner";

/** The icon family of a tool row (UI-21 "icon by tool type"). */
export type ToolIcon = "command" | "read" | "search" | "edit" | "web" | "agent" | "plugin" | "other";
export type ToolCallStatus = "pending" | "running" | "ok" | "error" | "denied";

const ICON: Record<ToolIcon, typeof FileText> = {
  command: SquareTerminal,
  read: FileText,
  search: Search,
  edit: Pencil,
  web: Globe,
  agent: Bot,
  plugin: Plug,
  other: Wrench,
};

export type ToolCallBlockProps = {
  icon: ToolIcon;
  /** The verb phrase, e.g. "Ran" or "Read". */
  verb: ReactNode;
  /** The mono argument: a path, a command or a query. */
  arg?: string;
  /** Trailing detail in fg-subtle, e.g. "exit 1, 6s" or "+12 −3". */
  meta?: ReactNode;
  status: ToolCallStatus;
  /** Spoken status, e.g. "Done", "Running", "Failed", "Blocked". */
  statusLabel: string;
  /** Output, diff or detail shown when the row is expanded. */
  children?: ReactNode;
  defaultOpen?: boolean;
  className?: string;
};

function StatusMark({ status, label }: { status: ToolCallStatus; label: string }) {
  if (status === "running" || status === "pending") return <Spinner size={12} label={label} />;
  const Icon = status === "ok" ? Check : status === "denied" ? Ban : TriangleAlert;
  return (
    <span role="img" aria-label={label} className="inline-flex">
      <Icon
        size={12}
        aria-hidden="true"
        className={cx(status === "ok" ? "text-done-text" : status === "denied" ? "text-fg-subtle" : "text-error")}
      />
    </span>
  );
}

/** One tool call in a transcript (UI-21): icon, verb and mono argument, a
 * trailing status mark; expandable when it has output. */
export function ToolCallBlock({ icon, verb, arg, meta, status, statusLabel, children, defaultOpen = false, className }: ToolCallBlockProps) {
  const [open, setOpen] = useState(defaultOpen);
  const panel = useId();
  const Icon = ICON[icon];
  const expandable = children !== undefined && children !== null && children !== false;
  const row = (
    <>
      <span aria-hidden="true" className="inline-flex w-3 shrink-0 justify-center text-fg-subtle">
        {expandable ? open ? <ChevronDown size={12} /> : <ChevronRight size={12} /> : null}
      </span>
      <Icon size={14} aria-hidden="true" className="shrink-0 text-fg-muted" />
      <span className="flex min-w-0 flex-1 items-baseline gap-1.5">
        <span className="shrink-0 text-fg-muted">{verb}</span>
        {arg && <span className="min-w-0 truncate font-mono text-sm text-fg">{arg}</span>}
        {meta && <span className="shrink-0 text-sm tabular-nums text-fg-subtle">{meta}</span>}
      </span>
      <StatusMark status={status} label={statusLabel} />
    </>
  );
  return (
    <div className={cx("flex flex-col", className)} data-tool-status={status}>
      {expandable ? (
        <button
          type="button"
          aria-expanded={open}
          aria-controls={panel}
          onClick={() => setOpen(!open)}
          className="flex h-7 w-full items-center gap-2 rounded-sm px-1 text-left text-base hover:bg-surface-3 focus-visible:shadow-focus focus-visible:outline-none"
        >
          {row}
        </button>
      ) : (
        <div className="flex h-7 items-center gap-2 px-1 text-base">{row}</div>
      )}
      {expandable && open && (
        <div id={panel} className="ml-6 mt-1 flex flex-col gap-2">
          {children}
        </div>
      )}
    </div>
  );
}

/** Mono output of a tool, scrollable, with an optional truncation note. */
export function ToolOutput({ text, note, className }: { text: string; note?: ReactNode; className?: string }) {
  return (
    <div className={cx("overflow-hidden rounded-md border border-border bg-surface-1", className)}>
      <pre className="max-h-64 overflow-auto p-2 font-mono text-sm leading-relaxed text-fg-muted">{text}</pre>
      {note && <p className="border-t border-divider px-2 py-1 text-2xs text-fg-subtle">{note}</p>}
    </div>
  );
}

export type ToolGroupProps = {
  /** "{n} tool calls". */
  summary: ReactNode;
  collapseLabel: string;
  /** The worst status in the group, for the trailing mark. */
  status: ToolCallStatus;
  statusLabel: string;
  children: ReactNode;
  defaultOpen?: boolean;
};

/** Three or more consecutive tool calls fold into one row (UI-21). */
export function ToolGroup({ summary, collapseLabel, status, statusLabel, children, defaultOpen = false }: ToolGroupProps) {
  const [open, setOpen] = useState(defaultOpen);
  const panel = useId();
  return (
    <div className="flex flex-col">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={panel}
        onClick={() => setOpen(!open)}
        className="flex h-7 w-full items-center gap-2 rounded-sm px-1 text-left text-base text-fg-muted hover:bg-surface-3 focus-visible:shadow-focus focus-visible:outline-none"
      >
        <span aria-hidden="true" className="inline-flex w-3 shrink-0 justify-center text-fg-subtle">
          {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </span>
        <Wrench size={14} aria-hidden="true" className="shrink-0" />
        <span className="min-w-0 flex-1 truncate">{open ? collapseLabel : summary}</span>
        <StatusMark status={status} label={statusLabel} />
      </button>
      {open && (
        <div id={panel} className="ml-3 flex flex-col border-l border-divider pl-2">
          {children}
        </div>
      )}
    </div>
  );
}
