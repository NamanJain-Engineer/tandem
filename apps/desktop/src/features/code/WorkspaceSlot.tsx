import { Badge, StatusDot, cx } from "@tandem/ui";
import { t } from "../../i18n";
import type { WorkspaceStatus } from "./workspaceStatus";

/** A rail row's status, quiet, never a box (D-2603): amber "Needs you", a
 * working dot and its count, or "Failed"; then the unread dot when something
 * finished unseen. Folded (D-2701), the words are their dot, the StatusDot of
 * their state with the words as its name and tooltip, and the unread dot sits
 * on it as it does on a pane's row. */
export function WorkspaceSlot({ status: s, fold = false }: { status: WorkspaceStatus; fold?: boolean }) {
  const words = s.waiting
    ? t("rail.waiting")
    : s.working > 0
      ? t("rail.working", { n: s.working })
      : s.failed
        ? t("rail.failed")
        : null;
  const unread = (className: string) => (
    <span
      data-testid="workspace-unread"
      role="img"
      aria-label={t("rail.unread")}
      className={cx("size-1.5 shrink-0 rounded-full bg-accent", className)}
    />
  );
  if (words && fold)
    return (
      <span
        data-testid="workspace-status"
        data-folded=""
        title={words}
        className="relative inline-flex shrink-0"
      >
        <StatusDot
          status={s.waiting ? "waiting" : s.working > 0 ? "working" : "error"}
          size={6}
          label={words}
        />
        {s.unread && unread("absolute -right-1 -top-1")}
      </span>
    );
  const slot = s.waiting ? (
    <Badge variant="waiting" quiet>
      {words}
    </Badge>
  ) : s.working > 0 ? (
    <span className="inline-flex items-center gap-1 text-xs tabular-nums text-fg-muted">
      <span aria-hidden="true" className="inline-flex">
        <StatusDot status="working" size={6} />
      </span>
      {words}
    </span>
  ) : s.failed ? (
    <Badge variant="error" quiet>
      {words}
    </Badge>
  ) : null;
  return (
    <>
      {slot && (
        <span data-testid="workspace-status" data-words="" className="inline-flex shrink-0">
          {slot}
        </span>
      )}
      {s.unread && unread("")}
    </>
  );
}
