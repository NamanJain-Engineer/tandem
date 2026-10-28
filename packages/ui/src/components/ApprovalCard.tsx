import { useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Check, Diamond, TriangleAlert, X } from "lucide-react";
import { cx } from "../cx";
import { Button } from "./Button";
import { useSteadyBox } from "./steadyBox";

/** ARCHITECTURE §6.9 `ApprovalKind`. */
export type ApprovalCardKind =
  | "command"
  | "file_write"
  | "file_delete"
  | "network"
  | "mcp"
  | "plugin_action"
  | "spend"
  | "question"
  | "other";

export type ApprovalCardLabels = {
  waiting: string;
  /** "Allow once" (commands), "Allow" (files), "Approve" (plugins, spend). */
  allow: string;
  allowThread: string;
  deny: string;
  denyExplain: string;
  /** The explain box's label and send button. */
  reason: string;
  sendReason: string;
  cancel: string;
  /** "View diff" for file kinds; "Show details" otherwise. */
  showDetail: string;
  hideDetail: string;
  /** Allow's title while it waits for the card to hold still. */
  steadying: string;
};

export const APPROVAL_LABELS: ApprovalCardLabels = {
  waiting: "Waiting for you",
  allow: "Allow once",
  allowThread: "Allow for this thread",
  deny: "Deny",
  denyExplain: "Deny and explain…",
  reason: "Tell the agent why",
  sendReason: "Deny",
  cancel: "Cancel",
  showDetail: "Show details",
  hideDetail: "Hide details",
  steadying: "Allow turns on once this has been still on screen for a moment.",
};

export type ApprovalCardProps = {
  kind: ApprovalCardKind;
  /** "Claude Code wants to run a command". */
  title: ReactNode;
  /** A code block, diff or key/value list. */
  detail?: ReactNode;
  /** File edits and long details start collapsed behind a toggle. */
  detailCollapsed?: boolean;
  /** "In: ~/code/shop · Branch: tandem/fix-auth". */
  context?: ReactNode;
  risk?: "low" | "medium" | "high";
  riskText?: ReactNode;
  /** "2m". */
  age?: ReactNode;
  /** Offers "Allow for this thread" (the request's options include it). */
  allowThread?: boolean;
  /** What's being asked (the request's id, with its detail when that can
   * change): a new one starts Allow's wait over, even on the same card. */
  requestKey?: string;
  onAllow: () => void;
  onAllowThread?: () => void;
  onDeny: (reason: string | null) => void;
  /** A one-line resolved state replaces the card ("✓ Allowed once · you · 10:42"). */
  resolved?: { tone: "allowed" | "denied"; text: ReactNode; action?: ReactNode };
  /** The pinned copy above a composer: title and actions only. */
  compact?: boolean;
  busy?: boolean;
  labels?: Partial<ApprovalCardLabels>;
  className?: string;
};

/** UI_SPEC §2.24 / UI-23. Keys while the card has focus: A or Mod+Enter
 * allow, Mod+Shift+Enter allow for this thread, D deny, E deny and explain,
 * Esc focuses Deny. The Allow buttons and keys answer only once the card's
 * buttons have held still and uncovered on screen for a moment, measured
 * again on the click or key itself, so a card that slid in, moved or was
 * covered under the pointer can't take a click meant for something else. */
export function ApprovalCard({
  kind,
  title,
  detail,
  detailCollapsed = kind === "file_write" || kind === "file_delete",
  context,
  risk = "low",
  riskText,
  age,
  allowThread = false,
  requestKey,
  onAllow,
  onAllowThread,
  onDeny,
  resolved,
  compact = false,
  busy = false,
  labels: partial,
  className,
}: ApprovalCardProps) {
  const labels = { ...APPROVAL_LABELS, ...partial };
  const [showDetail, setShowDetail] = useState(!detailCollapsed);
  const [explaining, setExplaining] = useState(false);
  const [reason, setReason] = useState("");
  const denyRef = useRef<HTMLButtonElement>(null);
  const reasonId = useId();
  const titleId = useId();
  const box = useSteadyBox(!resolved && !explaining, requestKey ?? null);

  if (resolved) {
    const Icon = resolved.tone === "allowed" ? Check : X;
    return (
      <div
        data-testid="approval-resolved"
        className={cx("flex h-7 items-center gap-2 px-1 text-sm text-fg-muted", className)}
      >
        <Icon
          size={12}
          aria-hidden="true"
          className={resolved.tone === "allowed" ? "text-done-text" : "text-error"}
        />
        <span className="min-w-0 flex-1 truncate">{resolved.text}</span>
        {resolved.action}
      </div>
    );
  }

  const offerThread = allowThread && !!onAllowThread && kind !== "spend";
  // A key answers only the card that has focus, and only once it's steady.
  const allowNow = (e: KeyboardEvent<HTMLElement>) =>
    e.currentTarget.contains(document.activeElement) && box.check();
  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    const target = e.target as HTMLElement;
    if (busy || target.tagName === "TEXTAREA" || target.tagName === "INPUT") return;
    const mod = e.metaKey || e.ctrlKey;
    const key = e.key.toLowerCase();
    if (e.key === "Enter" && mod && e.shiftKey) {
      if (offerThread) {
        e.preventDefault();
        if (allowNow(e)) onAllowThread?.();
      }
    } else if ((e.key === "Enter" && mod) || (key === "a" && !mod && !e.altKey)) {
      e.preventDefault();
      if (allowNow(e)) onAllow();
    } else if (key === "d" && !mod && !e.altKey) {
      e.preventDefault();
      onDeny(null);
    } else if (key === "e" && !mod && !e.altKey) {
      e.preventDefault();
      setExplaining(true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (explaining) setExplaining(false);
      denyRef.current?.focus();
    }
  };

  const sendReason = () => {
    const text = reason.trim();
    onDeny(text || null);
  };

  return (
    <section
      tabIndex={0}
      aria-labelledby={titleId}
      data-testid="approval-card"
      data-kind={kind}
      onKeyDown={onKeyDown}
      className={cx(
        "flex flex-col gap-2 rounded-md border border-waiting bg-surface-2 p-3 focus-visible:shadow-focus focus-visible:outline-none",
        compact && "gap-1.5 py-2",
        className,
      )}
    >
      <header className="flex items-center gap-2 text-sm">
        <Diamond size={12} aria-hidden="true" className="shrink-0 fill-waiting text-waiting" />
        <span className="font-medium text-waiting-text">{labels.waiting}</span>
        {age && <span className="ml-auto text-2xs tabular-nums text-fg-subtle">{age}</span>}
      </header>
      <p id={titleId} className="text-base font-medium text-fg">
        {title}
      </p>
      {!compact && detail && (
        <>
          {detailCollapsed && (
            <Button
              size="sm"
              variant="ghost"
              className="self-start"
              onClick={() => setShowDetail(!showDetail)}
              aria-expanded={showDetail}
            >
              {showDetail ? labels.hideDetail : labels.showDetail}
            </Button>
          )}
          {showDetail && <div className="min-w-0">{detail}</div>}
        </>
      )}
      {!compact && context && <p className="text-sm text-fg-muted">{context}</p>}
      {!compact && risk !== "low" && riskText && (
        <p
          className={cx(
            "flex items-center gap-1.5 rounded-sm px-2 py-1 text-sm",
            risk === "high" ? "bg-error-soft text-error" : "bg-waiting-soft text-waiting-text",
          )}
        >
          <TriangleAlert size={14} aria-hidden="true" className="shrink-0" />
          {riskText}
        </p>
      )}
      {explaining ? (
        <div className="flex flex-col gap-2">
          <label htmlFor={reasonId} className="text-sm font-medium text-fg">
            {labels.reason}
          </label>
          <textarea
            id={reasonId}
            autoFocus
            rows={2}
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                sendReason();
              } else if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                setExplaining(false);
              }
            }}
            className="resize-none rounded-sm border border-border bg-surface-4 px-2.5 py-1.5 text-base text-fg placeholder:text-fg-subtle focus-visible:border-accent focus-visible:shadow-focus focus-visible:outline-none"
          />
          <div className="flex gap-2">
            <Button size="sm" variant="danger" onClick={sendReason} disabled={busy}>
              {labels.sendReason}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setExplaining(false)}>
              {labels.cancel}
            </Button>
          </div>
        </div>
      ) : (
        <div ref={box.ref} className="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            variant="primary"
            onClick={() => box.check() && onAllow()}
            disabled={busy || !box.steady}
            title={box.steady ? undefined : labels.steadying}
            data-grant
            data-testid="approval-allow"
          >
            {labels.allow}
          </Button>
          {offerThread && (
            <Button
              size="sm"
              variant="secondary"
              onClick={() => box.check() && onAllowThread?.()}
              disabled={busy || !box.steady}
              title={box.steady ? undefined : labels.steadying}
              data-grant
              data-testid="approval-allow-thread"
            >
              {labels.allowThread}
            </Button>
          )}
          <Button
            ref={denyRef}
            size="sm"
            variant="secondary"
            onClick={() => onDeny(null)}
            disabled={busy}
            data-testid="approval-deny"
          >
            {labels.deny}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            className="ml-auto"
            onClick={() => setExplaining(true)}
            disabled={busy}
          >
            {labels.denyExplain}
          </Button>
        </div>
      )}
    </section>
  );
}
