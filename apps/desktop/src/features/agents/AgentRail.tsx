import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Bot, CalendarClock, ListTodo, MoreHorizontal, Pin, Plus } from "lucide-react";
import type { TeammateSummary } from "@tandem/protocol";
import {
  Avatar,
  Banner,
  ConfirmDialog,
  ContextMenu,
  IconButton,
  Input,
  Menu,
  cx,
  useToast,
  type MenuEntry,
  type Status,
} from "@tandem/ui";
import { t } from "../../i18n";
import { useSetting, useSettings } from "../../stores/settings";
import { useTeammates } from "../../stores/teammates";
import { useAgentPage, type AgentPage } from "../../stores/agentPage";
import { useRoutines } from "../../stores/routines";
import { age } from "../thread/groups";

/** The rail's status for a teammate: failed, waiting, working or idle. */
export function teammateStatus(s: TeammateSummary): Status {
  if (s.failed) return "error";
  if (s.waiting) return "waiting";
  if (s.working > 0) return "working";
  return "idle";
}

/** UI-70 trailing slot: Failed > Needs you > n working > age, as quiet
 * coloured text; the avatar's status mark carries the shape (D-2603). */
function Trailing({ s, now, selected }: { s: TeammateSummary; now: number; selected: boolean }) {
  if (s.failed)
    return (
      <span className="shrink-0 text-xs font-medium text-danger" data-testid="teammate-trailing">
        {t("agent.rail.failed")}
      </span>
    );
  if (s.waiting)
    return (
      <span className="shrink-0 text-xs font-medium text-waiting-text" data-testid="teammate-trailing">
        {t("agent.rail.waiting")}
      </span>
    );
  if (s.working > 0)
    return (
      <span className="shrink-0 text-xs tabular-nums text-working-text" data-testid="teammate-trailing">
        {t("agent.rail.working", { n: s.working })}
      </span>
    );
  return (
    // On the selected row's accent tint, subtle text falls below AA.
    <span
      className={cx("shrink-0 text-xs tabular-nums", selected ? "text-fg-muted" : "text-fg-subtle")}
      data-testid="teammate-trailing"
    >
      {s.lastAt ? age(s.lastAt, now) : ""}
    </span>
  );
}

function useActions(s: TeammateSummary, onRename: () => void, onDelete: () => void): MenuEntry[] {
  const update = useTeammates((st) => st.update);
  const duplicate = useTeammates((st) => st.duplicate);
  const newChat = useTeammates((st) => st.newChat);
  const { show } = useToast();
  const fail = (r: unknown) => {
    if (r && typeof r === "object" && "code" in r && "message" in r)
      show({ tone: "error", title: String(r.message) });
  };
  const tm = s.teammate;
  return [
    ...(s.chatCapable
      ? [
          {
            id: "chat",
            label: t("agent.rail.newChat", { name: tm.name }),
            onSelect: () => void newChat(tm.id).then(fail),
          },
        ]
      : []),
    {
      id: "pin",
      label: tm.pinned ? t("agent.rail.unpin") : t("agent.rail.pin"),
      onSelect: () => void update(tm.id, { pinned: !tm.pinned }).then(fail),
    },
    { id: "rename", label: t("agent.rail.rename"), kbd: ["F2"], onSelect: onRename },
    { id: "duplicate", label: t("agent.rail.duplicate"), onSelect: () => void duplicate(tm.id).then(fail) },
    { id: "delete", label: t("agent.rail.delete"), danger: true, onSelect: onDelete },
  ];
}

function Row({
  s,
  hero,
  selected: selectedId,
  now,
}: {
  s: TeammateSummary;
  hero: boolean;
  selected: boolean;
  now: number;
}) {
  // While a footer page (Tasks, Routines) is open, no teammate is "the page".
  const onTeammate = useAgentPage((p) => p.page === "teammate");
  const selected = selectedId && onTeammate;
  const select = useTeammates((st) => st.select);
  const update = useTeammates((st) => st.update);
  const remove = useTeammates((st) => st.remove);
  const newChat = useTeammates((st) => st.newChat);
  const { show } = useToast();
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState(s.teammate.name);
  const [confirm, setConfirm] = useState(false);
  const tm = s.teammate;
  const entries = useActions(
    s,
    () => {
      setName(tm.name);
      setRenaming(true);
    },
    () => setConfirm(true),
  );
  const save = () => {
    setRenaming(false);
    const next = name.trim();
    if (next && next !== tm.name)
      void update(tm.id, { name: next }).then(
        (r) => "code" in r && show({ tone: "error", title: r.message }),
      );
  };
  const status = teammateStatus(s);
  return (
    <li className="group relative">
      {renaming ? (
        <div className="px-1 py-0.5">
          <Input
            label={t("agent.rail.renameField", { name: tm.name })}
            hideLabel
            autoFocus
            value={name}
            maxLength={40}
            onChange={(e) => setName(e.target.value)}
            onBlur={save}
            onKeyDown={(e) => {
              if (e.key === "Enter") save();
              if (e.key === "Escape") setRenaming(false);
            }}
          />
        </div>
      ) : (
        <ContextMenu entries={entries}>
          <button
            type="button"
            data-testid={hero ? "teammate-hero" : "teammate-row"}
            data-teammate-id={tm.id}
            data-status={status}
            aria-current={selected ? "page" : undefined}
            title={tm.name}
            onClick={() => {
              select(tm.id);
              useAgentPage.getState().show("teammate");
            }}
            onKeyDown={(e) => {
              if (e.key === "F2") {
                e.preventDefault();
                setName(tm.name);
                setRenaming(true);
              }
            }}
            className={cx(
              "flex w-full select-none items-center gap-2 rounded-sm px-2 text-left text-fg outline-none hover:bg-surface-4 focus-visible:shadow-focus",
              hero ? "py-2" : "py-1.5",
              selected && "bg-accent-soft hover:bg-accent-soft",
            )}
          >
            <Avatar name={tm.name} id={tm.id} hue={tm.avatarHue} size={32} status={status} />
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="flex items-center gap-1.5">
                <span className="min-w-[min(8ch,100%)] flex-1 truncate text-base font-medium">{tm.name}</span>
                {!s.chatCapable && <span className="sr-only">{t("agent.rail.terminalOnly")}</span>}
                <span className="flex shrink-0 items-center group-hover:hidden group-focus-within:hidden">
                  <Trailing s={s} now={now} selected={selected} />
                </span>
              </span>
              {/* Every row says what it's doing (D-2603). */}
              <span
                className={cx("truncate text-sm", selected ? "text-fg-muted" : "text-fg-subtle")}
                data-testid="teammate-line"
              >
                {s.lastLine ?? t("agent.rail.noLine")}
              </span>
            </span>
          </button>
        </ContextMenu>
      )}
      {!renaming && (
        <span className="absolute right-1 top-1/2 hidden -translate-y-1/2 items-center gap-0.5 group-hover:flex group-focus-within:flex">
          {s.chatCapable && (
            <IconButton
              size="sm"
              icon={<Plus size={14} />}
              label={t("agent.rail.newChat", { name: tm.name })}
              onClick={() =>
                void newChat(tm.id).then((r) => "code" in r && show({ tone: "error", title: r.message }))
              }
            />
          )}
          <IconButton
            size="sm"
            icon={<Pin size={14} />}
            label={tm.pinned ? t("agent.rail.unpin") : t("agent.rail.pin")}
            aria-pressed={tm.pinned}
            onClick={() =>
              void update(tm.id, { pinned: !tm.pinned }).then(
                (r) => "code" in r && show({ tone: "error", title: r.message }),
              )
            }
          />
          <Menu
            align="end"
            trigger={
              <IconButton
                size="sm"
                icon={<MoreHorizontal size={14} />}
                label={t("agent.rail.more", { name: tm.name })}
              />
            }
            entries={entries}
          />
        </span>
      )}
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        danger
        title={t("agent.rail.deleteTitle", { name: tm.name })}
        body={t("agent.rail.deleteBody")}
        confirmLabel={t("agent.rail.deleteOk")}
        onConfirm={() => {
          setConfirm(false);
          void remove(tm.id).then((e) => e && show({ tone: "error", title: e.message }));
        }}
      />
    </li>
  );
}

/** UI-70: the pinned teammate as the hero tile, then every other teammate. */
export function AgentRail({ collapsed, actions }: { collapsed: boolean; actions?: React.ReactNode }) {
  const list = useTeammates((s) => s.list);
  const loaded = useTeammates((s) => s.loaded);
  const error = useTeammates((s) => s.error);
  const selectedId = useTeammates((s) => s.selectedId);
  const select = useTeammates((s) => s.select);
  const refresh = useTeammates((s) => s.refresh);
  const [now, setNow] = useState(() => Date.now());
  const listRef = useRef<HTMLUListElement>(null);
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(id);
  }, []);
  const hero = list.find((s) => s.teammate.pinned) ?? null;
  const rest = list.filter((s) => s !== hero);

  // Arrow keys move between rows (standard rail keys).
  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const rows = Array.from(
      e.currentTarget.querySelectorAll<HTMLButtonElement>(
        '[data-testid="teammate-hero"],[data-testid="teammate-row"]',
      ),
    );
    const at = rows.indexOf(document.activeElement as HTMLButtonElement);
    if (at < 0) return;
    e.preventDefault();
    rows[Math.min(rows.length - 1, Math.max(0, at + (e.key === "ArrowDown" ? 1 : -1)))]?.focus();
  };

  if (collapsed) {
    return (
      <nav
        aria-label={t("rail.teammates")}
        className="flex w-[var(--rail-collapsed)] shrink-0 flex-col items-center gap-1 border-r border-border bg-surface-1 py-2"
      >
        {list.map((s) => (
          <button
            key={s.teammate.id}
            type="button"
            title={s.teammate.name}
            aria-label={s.teammate.name}
            aria-current={s.teammate.id === selectedId ? "page" : undefined}
            onClick={() => {
              select(s.teammate.id);
              useAgentPage.getState().show("teammate");
            }}
            className="rounded-md focus-visible:shadow-focus focus-visible:outline-none"
          >
            <Avatar
              name={s.teammate.name}
              id={s.teammate.id}
              hue={s.teammate.avatarHue}
              size={24}
              status={teammateStatus(s)}
            />
          </button>
        ))}
      </nav>
    );
  }

  return (
    <nav
      aria-label={t("rail.teammates")}
      data-testid="agent-rail"
      className="flex w-[var(--rail-w)] shrink-0 flex-col border-r border-border bg-surface-1"
      onKeyDown={onKeyDown}
    >
      <header className="flex h-9 items-center justify-between pl-3 pr-1.5">
        <h2 className="text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
          {t("rail.teammates").toUpperCase()}
        </h2>
        <span className="flex items-center gap-0.5">
          {actions}
          <RailMenu />
        </span>
      </header>
      <MailPaused />
      <div className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2">
        {error && !loaded ? (
          <div className="flex flex-col items-start gap-2 px-2 py-3 text-sm text-fg-muted">
            <p>{error.message}</p>
            <button type="button" className="text-accent underline" onClick={() => void refresh()}>
              {t("agent.rail.retry")}
            </button>
          </div>
        ) : !loaded ? (
          <p className="px-2 py-3 text-sm text-fg-muted">{t("agent.rail.loading")}</p>
        ) : list.length === 0 ? (
          <div className="flex flex-col items-center gap-2 px-3 py-6 text-center">
            <Bot size={20} aria-hidden="true" className="text-fg-subtle" />
            <p className="text-sm text-fg-muted">{t("agent.rail.empty")}</p>
          </div>
        ) : (
          <ul ref={listRef} className="flex flex-col gap-0.5" data-testid="teammate-list">
            {hero && <Row s={hero} hero selected={hero.teammate.id === selectedId} now={now} />}
            {rest.map((s) => (
              <Row key={s.teammate.id} s={s} hero={false} selected={s.teammate.id === selectedId} now={now} />
            ))}
          </ul>
        )}
      </div>
      <RailFooter />
    </nav>
  );
}

/** UI-70 footer: the pages beside the teammates. */
function RailFooter() {
  const page = useAgentPage((s) => s.page);
  const active = useRoutines(
    (s) => s.list.filter((v) => !v.routine.paused && !v.routine.pausedByPlan).length,
  );
  useEffect(() => {
    void useRoutines.getState().load();
  }, []);
  const rows: Array<{ id: AgentPage; label: string; icon: React.ReactNode; count?: number }> = [
    { id: "tasks", label: t("agent.footer.tasks"), icon: <ListTodo size={14} aria-hidden="true" /> },
    {
      id: "routines",
      label: t("agent.footer.routines"),
      icon: <CalendarClock size={14} aria-hidden="true" />,
      count: active,
    },
  ];
  return (
    <div
      className="flex shrink-0 flex-col gap-0.5 border-t border-divider p-1.5"
      data-testid="agent-rail-footer"
    >
      {rows.map((r) => (
        <button
          key={r.id}
          type="button"
          aria-current={page === r.id ? "page" : undefined}
          onClick={() => useAgentPage.getState().show(r.id)}
          data-testid={`agent-footer-${r.id}`}
          className={cx(
            "flex h-8 items-center gap-2 rounded-sm px-2 text-left text-sm text-fg-muted outline-none hover:bg-surface-4 hover:text-fg focus-visible:shadow-focus",
            page === r.id && "bg-accent-soft text-fg hover:bg-accent-soft",
          )}
        >
          {r.icon}
          <span className="flex-1">{r.label}</span>
          {!!r.count && <span className="text-xs text-fg-muted">{r.count}</span>}
        </button>
      ))}
    </div>
  );
}

/** UI-70 header ⋯: "Pause teammate mail". */
function RailMenu() {
  const paused = useSetting<boolean>("pause_all_mail", false);
  return (
    <Menu
      align="end"
      trigger={<IconButton size="sm" icon={<MoreHorizontal size={14} />} label={t("agent.rail.menu")} />}
      entries={[
        {
          type: "checkbox",
          id: "pause-mail",
          label: t("agent.mail.pauseToggle"),
          checked: paused,
          onCheckedChange: (on) => void useSettings.getState().set("pause_all_mail", on),
        },
      ]}
    />
  );
}

/** UI-107: while "Pause all teammate mail" is on, the rail says so. */
function MailPaused() {
  const paused = useSetting<boolean>("pause_all_mail", false);
  if (!paused) return null;
  return (
    <div className="px-1.5 pb-1.5" data-testid="mail-paused">
      <Banner
        tone="warning"
        className="rounded-md"
        action={{
          label: t("agent.mail.resume"),
          onClick: () => void useSettings.getState().set("pause_all_mail", false),
        }}
      >
        {t("agent.mail.paused")}
      </Banner>
    </div>
  );
}
