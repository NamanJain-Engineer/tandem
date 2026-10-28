import { useEffect, useMemo, useState } from "react";
import { Bell, Check } from "lucide-react";
import type { Notification } from "@tandem/protocol";
import {
  Button,
  EmptyState,
  IconButton,
  Popover,
  SegmentedControl,
  StatusDot,
  cx,
  useToast,
  type Status,
} from "@tandem/ui";
import { t } from "../../i18n";
import { useSteadyBox } from "../secrets/ApprovalGuard";
import { focusSource } from "../../stores/focus";
import { useNotifications } from "../../stores/notifications";
import { answer } from "../dashboard/actions";
import { useShortcutKeys } from "../commands/useShortcutKeys";

const WAITING_KINDS = new Set(["approval", "question"]);

/** Needs you: an unresolved approval or question. */
export const isWaiting = (n: Notification) => WAITING_KINDS.has(n.kind) && n.state !== "resolved";

function dot(n: Notification): Status | null {
  if (WAITING_KINDS.has(n.kind)) return n.state === "resolved" ? "done" : "waiting";
  if (n.kind === "done") return "done";
  if (n.kind === "failed") return "error";
  return null;
}

/** "now", "4m", "2h", "3d". */
export function ago(ms: number, now: number): string {
  const m = Math.max(0, Math.floor((now - ms) / 60_000));
  if (m < 1) return t("time.now");
  if (m < 60) return t("time.minutes", { n: m });
  const h = Math.floor(m / 60);
  return h < 24 ? t("time.hours", { n: h }) : t("time.days", { n: Math.floor(h / 24) });
}

function Item({ n, now, onJump }: { n: Notification; now: number; onJump: (n: Notification) => void }) {
  const { show } = useToast();
  const approvalId =
    n.kind === "approval" && n.state !== "resolved" && n.dedupeKey?.startsWith("approval:")
      ? n.dedupeKey.slice(9)
      : null;
  const status = dot(n);
  // New items arrive on top and push this one down: Allow once waits until
  // its own place has held still, and checks again at the click (F-TL-12.12).
  const box = useSteadyBox(approvalId !== null);
  const decide = (allow: boolean) =>
    approvalId &&
    void answer(approvalId, allow, "notification").then(
      (err) => err && show({ tone: "error", title: err.message }),
    );
  return (
    <li
      data-testid="notification"
      data-kind={n.kind}
      data-state={n.state}
      tabIndex={0}
      onClick={() => onJump(n)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && e.target === e.currentTarget) {
          e.preventDefault();
          onJump(n);
        }
      }}
      className="flex cursor-pointer gap-2.5 rounded-sm px-2.5 py-2 outline-none hover:bg-surface-4 focus-visible:shadow-focus"
    >
      <span className="mt-1 inline-flex size-3 shrink-0 items-center justify-center">
        {status ? (
          <StatusDot status={status} size={8} />
        ) : (
          <Bell size={12} aria-hidden="true" className="text-fg-muted" />
        )}
      </span>
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex items-center gap-2">
          <span
            className={cx(
              "min-w-0 flex-1 truncate text-base",
              n.state === "unread" ? "font-medium text-fg" : "text-fg-muted",
            )}
          >
            {n.title}
          </span>
          <span className="shrink-0 text-sm tabular-nums text-fg-subtle">{ago(n.createdAt, now)}</span>
          {n.state === "unread" && (
            <span
              role="img"
              aria-label={t("notifications.unread")}
              className="size-1.5 shrink-0 rounded-full bg-accent"
            />
          )}
        </span>
        {n.body && <span className="truncate text-sm text-fg-muted">{n.body}</span>}
        {WAITING_KINDS.has(n.kind) && n.state === "resolved" && (
          <span className="flex items-center gap-1 text-sm text-fg-subtle">
            <Check size={12} aria-hidden="true" />
            {t("notifications.resolved")}
          </span>
        )}
        {approvalId && (
          <span ref={box.ref} className="flex gap-2 pt-1" onClick={(e) => e.stopPropagation()}>
            <Button
              size="sm"
              variant="primary"
              data-testid="notification-allow"
              disabled={!box.steady}
              title={box.steady ? undefined : t("approval.waiting")}
              onClick={() => box.check() && decide(true)}
            >
              {t("dashboard.allowOnce")}
            </Button>
            <Button size="sm" variant="secondary" onClick={() => decide(false)}>
              {t("dashboard.deny")}
            </Button>
          </span>
        )}
      </span>
    </li>
  );
}

/** UI-40 / F-DB-02: the bell and its "Waiting on you" popover. */
export function NotificationsBell() {
  const keys = useShortcutKeys();
  const { items, unread, loaded, nextCursor, open, setOpen } = useNotifications();
  const waiting = useMemo(() => items.filter(isWaiting), [items]);
  const [tab, setTab] = useState<"waiting" | "all">("all");
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!loaded) void useNotifications.getState().load();
  }, [loaded]);
  useEffect(() => {
    if (!open) return;
    setNow(Date.now());
    setTab(waiting.length > 0 ? "waiting" : "all");
    // Only when it opens: switching tabs is the user's.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // Mark read and jump; the popover closes so the source is in view
  // (F-DB-02: the clicked item opens reliably even in a burst).
  const jump = (n: Notification) => {
    setOpen(false);
    if (n.state === "unread") void useNotifications.getState().markRead([n.id]);
    void focusSource(n.source);
  };

  const shown = tab === "waiting" ? waiting : items;
  return (
    <Popover
      label={t("notifications.title")}
      open={open}
      onOpenChange={setOpen}
      className="w-[400px]"
      trigger={
        <IconButton
          icon={<Bell size={16} />}
          label={unread > 0 ? t("notifications.bellUnread", { n: unread }) : t("titlebar.notifications")}
          badge={unread > 0 ? unread : undefined}
          badgeTone={waiting.some((n) => n.state === "unread") ? "waiting" : "neutral"}
          kbd={keys("notifications.open")}
          data-testid="notifications-bell"
        />
      }
    >
      <div data-testid="notifications-popover" className="flex min-h-0 flex-col">
        <div className="flex items-center gap-2 border-b border-divider px-3 py-2">
          <h2 className="flex-1 text-md font-semibold text-fg">{t("notifications.title")}</h2>
          <Button
            size="sm"
            variant="ghost"
            disabled={unread === 0}
            onClick={() => void useNotifications.getState().markAllRead()}
          >
            {t("notifications.markAllRead")}
          </Button>
        </div>
        <div className="px-3 pt-2">
          <SegmentedControl
            size="sm"
            label={t("notifications.show")}
            value={tab}
            onValueChange={(v) => setTab(v === "waiting" ? "waiting" : "all")}
            items={[
              { id: "waiting", label: t("notifications.waitingTab", { n: waiting.length }) },
              { id: "all", label: t("notifications.allTab") },
            ]}
          />
        </div>
        {shown.length === 0 ? (
          <EmptyState
            icon={<Bell size={20} />}
            title={t("notifications.emptyTitle")}
            body={t("notifications.emptyBody")}
          />
        ) : (
          <ul className="min-h-0 flex-1 overflow-auto p-1.5">
            {shown.map((n) => (
              <Item key={n.id} n={n} now={now} onJump={jump} />
            ))}
            {tab === "all" && nextCursor && (
              <li className="flex justify-center p-2">
                <Button size="sm" variant="ghost" onClick={() => void useNotifications.getState().loadMore()}>
                  {t("notifications.more")}
                </Button>
              </li>
            )}
          </ul>
        )}
      </div>
    </Popover>
  );
}
