import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type DragEvent,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { Bot, CalendarClock, Home, MessagesSquare, Plus, Settings2 } from "lucide-react";
import type { IpcError, PluginCard, TeammateSummary, Thread, ThreadState } from "@tandem/protocol";
import {
  Avatar,
  Banner,
  Button,
  ConfirmDialog,
  ContextMenu,
  EmptyState,
  EngineBadge,
  Input,
  Menu,
  StatusDot,
  Tabs,
  cx,
  useToast,
  type MenuEntry,
  type Status,
} from "@tandem/ui";
import { t } from "../../i18n";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import { useAgentPage } from "../../stores/agentPage";
import { useTeammates } from "../../stores/teammates";
import { useThreads } from "../../stores/threads";
import { switchMode } from "../shell/actions";
import { ChatView } from "../thread/ThreadMain";
import { teammateStatus } from "./AgentRail";
import { mailMention } from "./mention";
import { MemoryTab } from "./MemoryTab";
import { SkillsTab } from "./SkillsTab";
import { SuggestionCards } from "./Suggestions";

const CHAT_DOT: Record<ThreadState, Status> = {
  idle: "idle",
  working: "working",
  waiting: "waiting",
  done: "done",
  failed: "error",
  interrupted: "idle",
};

export type PageTab = "chats" | "memory" | "skills" | "settings";

function baseName(path: string): string {
  return (
    path
      .replace(/[\\/]+$/, "")
      .split(/[\\/]/)
      .pop() ?? path
  );
}

/** "⌂ Home" or the place's folder name. */
function placeLabel(cwd: string, home: string): string {
  return cwd === home ? t("agent.chat.home") : baseName(cwd);
}

/** F-AG-03: connected plugins this teammate doesn't have; Enable grants them. */
function PluginNudge({ s }: { s: TeammateSummary }) {
  const version = useTeammates((st) => st.version);
  const [off, setOff] = useState<PluginCard[]>([]);
  const { show } = useToast();
  const tm = s.teammate;
  const load = useCallback(async () => {
    try {
      const [cards, granted] = await Promise.all([
        invoke<PluginCard[]>("plugins_catalog"),
        invoke<string[]>("grants_list", { subject: { type: "teammate", teammateId: tm.id } }),
      ]);
      setOff(
        cards.filter((c) => c.state === "connected" && c.connectionId && !granted.includes(c.connectionId)),
      );
    } catch {
      setOff([]);
    }
  }, [tm.id]);
  useEffect(() => {
    void load();
  }, [load, version]);
  // A plugin connected or changed (F-PL-03).
  useEffect(() => {
    let stop: (() => void) | undefined;
    let gone = false;
    void listen<PluginCard>("plugins:changed", () => void load()).then((u) => {
      if (gone) u();
      else stop = u;
    });
    return () => {
      gone = true;
      stop?.();
    };
  }, [load]);
  if (off.length === 0) return null;
  const enable = async () => {
    try {
      for (const c of off) {
        await invoke("grants_set", {
          subject: { type: "teammate", teammateId: tm.id },
          connectionId: c.connectionId,
          granted: true,
        });
      }
      await load();
    } catch (e) {
      show({ tone: "error", title: (e as IpcError).message });
    }
  };
  return (
    <div data-testid="plugin-nudge">
      <Banner
        tone="info"
        className="rounded-md"
        action={{ label: t("agent.nudge.enable"), onClick: () => void enable() }}
      >
        {t("agent.nudge.text", { plugins: off.map((c) => c.label).join(", "), name: tm.name })}
      </Banner>
    </div>
  );
}

/** A chat's tab: status dot and title; right-click Rename / Close / Delete. */
function ChatPill({
  chat,
  selected,
  onSelect,
  onDragStart,
  onDrop,
  teammateId,
}: {
  chat: Thread;
  selected: boolean;
  onSelect: () => void;
  onDragStart: (e: DragEvent) => void;
  onDrop: (e: DragEvent) => void;
  teammateId: string;
}) {
  const update = useThreads((s) => s.update);
  const loadChats = useTeammates((s) => s.loadChats);
  const { show } = useToast();
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState(chat.title);
  const title = chat.title || t("agent.chat.untitled");
  const fail = (e: unknown) => show({ tone: "error", title: (e as IpcError).message });
  const close = async () => {
    await invoke("thread_archive", { threadId: chat.id, archived: true }).catch(fail);
    await loadChats(teammateId);
  };
  const entries: MenuEntry[] = [
    { id: "rename", label: t("agent.chat.rename"), kbd: ["F2"], onSelect: () => setRenaming(true) },
    { id: "close", label: t("agent.chat.close"), onSelect: () => void close() },
    {
      id: "delete",
      label: t("agent.chat.delete"),
      danger: true,
      onSelect: () =>
        void invoke("thread_archive", { threadId: chat.id, archived: true })
          .then(() => invoke("thread_delete", { threadId: chat.id }))
          .then(() => loadChats(teammateId), fail),
    },
  ];
  if (renaming) {
    return (
      <Input
        label={t("agent.chat.renameField", { name: title })}
        hideLabel
        autoFocus
        value={name}
        className="w-40"
        onChange={(e) => setName(e.target.value)}
        onBlur={() => {
          setRenaming(false);
          if (name.trim() && name.trim() !== chat.title)
            void update(chat.id, { title: name.trim() }).then(() => loadChats(teammateId));
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape") setRenaming(false);
        }}
      />
    );
  }
  return (
    <ContextMenu entries={entries}>
      <button
        type="button"
        role="tab"
        aria-selected={selected}
        tabIndex={selected ? 0 : -1}
        data-testid="chat-tab"
        data-chat-id={chat.id}
        draggable
        onDragStart={onDragStart}
        onDragOver={(e) => e.preventDefault()}
        onDrop={onDrop}
        onClick={onSelect}
        onKeyDown={(e) => {
          if (e.key === "F2") {
            e.preventDefault();
            setRenaming(true);
          }
        }}
        title={title}
        className={cx(
          "inline-flex h-7 max-w-48 shrink-0 items-center gap-1.5 rounded-sm px-2.5 text-base text-fg-muted hover:text-fg focus-visible:shadow-focus focus-visible:outline-none",
          selected && "bg-surface-4 text-fg",
        )}
      >
        <StatusDot status={CHAT_DOT[chat.state]} label={t(`thread.state.${chat.state}`)} />
        <span className="truncate">{title}</span>
      </button>
    </ContextMenu>
  );
}

const DRAG_MIME = "application/x-vibezeo-chat";

function ChatsTab({ s, visible }: { s: TeammateSummary; visible: boolean }) {
  const tm = s.teammate;
  const chats = useTeammates((st) => st.chats[tm.id]);
  const chatId = useTeammates((st) => st.chatOf[tm.id] ?? null);
  const selectChat = useTeammates((st) => st.selectChat);
  const newChat = useTeammates((st) => st.newChat);
  const loadChats = useTeammates((st) => st.loadChats);
  const { show } = useToast();
  const [chips, setChips] = useState<string[]>([]);
  useEffect(() => {
    void invoke<string[]>("teammate_suggestions", { id: tm.id }).then(setChips, () => setChips([]));
  }, [tm.id, tm.brief]);
  useEffect(() => {
    if (!chats) void loadChats(tm.id);
  }, [chats, loadChats, tm.id]);

  const start = useCallback(
    async (place?: string | null) => {
      const r = await newChat(tm.id, place ?? null);
      if ("code" in r) show({ tone: "error", title: r.message });
    },
    [newChat, show, tm.id],
  );

  // An empty chat list starts a chat on the first message.
  const [starting, setStarting] = useState(false);

  if (!s.chatCapable) {
    const label = useEngines.getState().label(tm.engineId) ?? tm.engineId;
    return (
      <div className="flex flex-col gap-3 p-4">
        <Banner tone="info" action={{ label: t("agent.chat.openCode"), onClick: () => switchMode("code") }}>
          {t("agent.chat.terminalOnly", { engine: label })}
        </Banner>
      </div>
    );
  }

  const order = (chats ?? []).map((c) => c.id);
  const reorder = (from: string, to: string) => {
    if (from === to) return;
    const next = order.filter((id) => id !== from);
    next.splice(next.indexOf(to) + (order.indexOf(from) < order.indexOf(to) ? 1 : 0), 0, from);
    useTeammates.setState((st) => ({
      chats: {
        ...st.chats,
        [tm.id]: next
          .map((id) => (st.chats[tm.id] ?? []).find((c) => c.id === id))
          .filter((c): c is Thread => !!c),
      },
    }));
    void invoke("thread_reorder", { ids: next }).catch((e: IpcError) =>
      show({ tone: "error", title: e.message }),
    );
  };
  const onStripKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const at = chatId ? order.indexOf(chatId) : -1;
    const next = order[Math.min(order.length - 1, Math.max(0, at + (e.key === "ArrowRight" ? 1 : -1)))];
    if (!next) return;
    e.preventDefault();
    selectChat(tm.id, next);
    const strip = e.currentTarget;
    requestAnimationFrame(() => strip.querySelector<HTMLButtonElement>(`[data-chat-id="${next}"]`)?.focus());
  };
  const placeEntries: MenuEntry[] = [
    {
      id: "home",
      label: t("agent.chat.newIn", { place: t("agent.chat.home") }),
      onSelect: () => void start(null),
    },
    ...tm.places.map((p) => ({
      id: p,
      label: t("agent.chat.newIn", { place: baseName(p) }),
      onSelect: () => void start(p),
    })),
  ];
  const open = (chats ?? []).find((c) => c.id === chatId) ?? null;
  const emptyState = (prefill: (text: string) => void): ReactNode => (
    <EmptyState
      icon={<MessagesSquare size={20} />}
      title={t("agent.chat.emptyTitle", { name: tm.name })}
      body={t("agent.chat.emptyBody", { name: tm.name })}
      primary={
        <span className="flex flex-wrap justify-center gap-2" data-testid="suggestion-chips">
          {chips.map((c) => (
            <Button key={c} size="sm" variant="secondary" onClick={() => prefill(c)}>
              {c}
            </Button>
          ))}
        </span>
      }
    />
  );
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div
        role="tablist"
        aria-label={t("agent.chat.tabs", { name: tm.name })}
        className="flex shrink-0 items-center gap-1 overflow-x-auto border-b border-divider px-3 py-1"
        onKeyDown={onStripKey}
        data-testid="chat-tabs"
      >
        {(chats ?? []).map((c) => (
          <ChatPill
            key={c.id}
            chat={c}
            teammateId={tm.id}
            selected={c.id === chatId}
            onSelect={() => selectChat(tm.id, c.id)}
            onDragStart={(e) => {
              e.dataTransfer.setData(DRAG_MIME, c.id);
              e.dataTransfer.effectAllowed = "move";
            }}
            onDrop={(e) => {
              const from = e.dataTransfer.getData(DRAG_MIME);
              if (from) {
                e.preventDefault();
                reorder(from, c.id);
              }
            }}
          />
        ))}
        {tm.places.length > 0 ? (
          <Menu
            trigger={
              <Button size="sm" variant="ghost" iconLeft={<Plus size={14} />} data-testid="chat-new">
                {t("agent.chat.new")}
              </Button>
            }
            entries={placeEntries}
          />
        ) : (
          <Button
            size="sm"
            variant="ghost"
            iconLeft={<Plus size={14} />}
            onClick={() => void start(null)}
            data-testid="chat-new"
          >
            {t("agent.chat.new")}
          </Button>
        )}
      </div>
      <div className="min-h-0 flex-1">
        {open ? (
          <ChatView
            key={open.id}
            threadId={open.id}
            visible={visible}
            chat={{
              placeholder: t("agent.chat.placeholder", { name: tm.name }),
              empty: emptyState,
              beforeSend: async (text) => {
                const all = useTeammates.getState().list.map((x) => x.teammate);
                const m = mailMention(text, all, tm.id);
                if (!m) return false;
                await invoke("mail_send", {
                  fromTeammateId: tm.id,
                  toTeammateId: m.to.id,
                  body: m.body,
                  fromThreadId: open.id,
                });
                return true;
              },
              above: (
                <>
                  <SuggestionCards threadId={open.id} />
                  <PluginNudge s={s} />
                </>
              ),
              header: (
                <div
                  className="flex h-7 shrink-0 items-center gap-1.5 border-b border-divider px-3 text-sm text-fg-muted"
                  data-testid="chat-place"
                >
                  <Home size={12} aria-hidden="true" />
                  {placeLabel(open.cwd, tm.home)}
                </div>
              ),
            }}
          />
        ) : chats ? (
          <div className="flex h-full flex-col items-center justify-center gap-3">
            {emptyState(() => undefined)}
            <Button
              variant="primary"
              disabled={starting}
              onClick={() => {
                setStarting(true);
                void start(null).finally(() => setStarting(false));
              }}
              data-testid="chat-start"
            >
              {t("agent.chat.start", { name: tm.name })}
            </Button>
            <div className="w-full max-w-3xl px-4">
              <PluginNudge s={s} />
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

/** UI-71: a teammate's page (header, then Chats, Memory, Skills, Settings). */
export function TeammatePage({
  s,
  visible,
  settings,
}: {
  s: TeammateSummary;
  visible: boolean;
  settings?: ReactNode;
}) {
  const tm = s.teammate;
  const engine = useEngines((st) => st.list.find((e) => e.spec.id === tm.engineId));
  const chats = useTeammates((st) => st.chats[tm.id]?.length ?? 0);
  const newChat = useTeammates((st) => st.newChat);
  const [tab, setTabNow] = useState<PageTab>("chats");
  const [leaving, setLeaving] = useState<PageTab | null>(null);
  const dirty = useTeammates((st) => st.dirty);
  // Unsaved settings: ask before leaving the tab (UI-75's guard).
  const setTab = (next: PageTab) => {
    if (tab === "settings" && next !== "settings" && dirty) setLeaving(next);
    else setTabNow(next);
  };
  const { show } = useToast();
  const status = teammateStatus(s);
  const pill =
    status === "waiting"
      ? t("agent.page.waiting")
      : s.working > 0
        ? t("agent.page.working", { n: s.working })
        : t("agent.page.ready");
  const tabs = useMemo(
    () => [
      { id: "chats", label: t("agent.page.chats"), count: chats },
      { id: "memory", label: t("agent.page.memory") },
      { id: "skills", label: t("agent.page.skills") },
      ...(settings ? [{ id: "settings", label: t("agent.page.settings") }] : []),
    ],
    [chats, settings],
  );
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.altKey && /^[1-4]$/.test(e.key)) {
      const next = tabs[Number(e.key) - 1];
      if (next) {
        e.preventDefault();
        setTab(next.id as PageTab);
      }
    } else if (mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "n" && s.chatCapable) {
      e.preventDefault();
      setTab("chats");
      void newChat(tm.id).then((r) => "code" in r && show({ tone: "error", title: r.message }));
    }
  };
  return (
    <div
      className="flex h-full min-h-0 flex-col"
      data-testid="teammate-page"
      data-teammate-id={tm.id}
      onKeyDown={onKeyDown}
    >
      <header className="flex shrink-0 items-start gap-3 border-b border-border bg-surface-2 px-4 py-3">
        <Avatar name={tm.name} id={tm.id} hue={tm.avatarHue} size={48} status={status} />
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <h1 className="truncate text-lg font-semibold text-fg" data-testid="teammate-name">
            {tm.name}
          </h1>
          <p className="truncate text-base text-fg-muted">{tm.brief.split("\n")[0]}</p>
          <span className="flex items-center gap-1 text-sm text-fg-muted">
            {t("agent.page.poweredBy")}
            <EngineBadge
              letters={engine?.spec.logo ?? ""}
              name={engine?.spec.label ?? tm.engineId}
              size={14}
            />
          </span>
        </div>
        <span
          className="inline-flex shrink-0 items-center gap-1.5 text-sm text-fg-muted"
          data-testid="teammate-status"
        >
          <StatusDot status={status} />
          {pill}
        </span>
        {s.chatCapable && (
          <Button
            size="sm"
            variant="secondary"
            iconLeft={<Plus size={14} />}
            onClick={() => {
              setTab("chats");
              void newChat(tm.id).then((r) => "code" in r && show({ tone: "error", title: r.message }));
            }}
          >
            {t("agent.page.newChat")}
          </Button>
        )}
        <Button
          size="sm"
          variant="ghost"
          iconLeft={<CalendarClock size={14} />}
          onClick={() => useAgentPage.getState().show("routines")}
          data-testid="teammate-routines"
        >
          {t("agent.page.routines")}
        </Button>
        {settings && (
          <Button
            size="sm"
            variant="ghost"
            iconLeft={<Settings2 size={14} />}
            onClick={() => setTab("settings")}
            aria-label={t("agent.page.gear")}
          >
            {t("agent.page.settings")}
          </Button>
        )}
      </header>
      <Tabs
        className="min-h-0 flex-1 px-4"
        label={t("agent.page.tabs", { name: tm.name })}
        value={tab}
        onValueChange={(v) => setTab(v as PageTab)}
        items={tabs}
        renderPanel={(id) =>
          id === "chats" ? (
            <ChatsTab s={s} visible={visible && tab === "chats"} />
          ) : id === "memory" ? (
            <MemoryTab teammate={tm} />
          ) : id === "skills" ? (
            <SkillsTab teammate={tm} />
          ) : (
            settings
          )
        }
      />
      <ConfirmDialog
        open={leaving !== null}
        onOpenChange={(o) => !o && setLeaving(null)}
        title={t("agent.settings.leaveTitle")}
        body={t("agent.settings.leaveBody")}
        confirmLabel={t("agent.settings.leaveOk")}
        onConfirm={() => {
          useTeammates.getState().setDirty(false);
          if (leaving) setTabNow(leaving);
          setLeaving(null);
        }}
      />
    </div>
  );
}

/** Agent mode's main area: the open teammate, else a prompt to pick one. */
export function AgentMain({
  visible,
  settingsFor,
}: {
  visible: boolean;
  settingsFor?: (s: TeammateSummary) => ReactNode;
}) {
  const list = useTeammates((s) => s.list);
  const loaded = useTeammates((s) => s.loaded);
  const selectedId = useTeammates((s) => s.selectedId);
  const select = useTeammates((s) => s.select);
  const open = list.find((s) => s.teammate.id === selectedId) ?? null;
  const pendingId = useTeammates((s) => s.pendingId);
  const resolvePending = useTeammates((s) => s.resolvePending);
  // The hero opens by default.
  useEffect(() => {
    if (!selectedId && loaded && list.length) {
      const hero = list.find((s) => s.teammate.pinned) ?? list[0];
      if (hero) select(hero.teammate.id);
    }
  }, [selectedId, loaded, list, select]);
  if (!open) {
    return (
      <div className="flex h-full items-center justify-center">
        <EmptyState
          icon={<Bot size={20} />}
          title={t("agent.main.emptyTitle")}
          body={t("agent.main.emptyBody")}
        />
      </div>
    );
  }
  return (
    <>
      <TeammatePage key={open.teammate.id} s={open} visible={visible} settings={settingsFor?.(open)} />
      <ConfirmDialog
        open={pendingId !== null}
        onOpenChange={(o) => !o && resolvePending(false)}
        title={t("agent.settings.leaveTitle")}
        body={t("agent.settings.leaveBody")}
        confirmLabel={t("agent.settings.leaveOk")}
        onConfirm={() => resolvePending(true)}
      />
    </>
  );
}
