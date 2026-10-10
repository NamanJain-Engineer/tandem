import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type HTMLAttributes,
  type ReactNode,
} from "react";
import { Bot, FolderOpen, GitBranch, Home, MessagesSquare, X } from "lucide-react";
import type { IpcError, PlanRevision, Thread } from "@tandem/protocol";
import {
  ApprovalCard,
  Banner,
  Button,
  Chip,
  Composer,
  ContextMeter,
  Dialog,
  EmptyState,
  EngineBadge,
  IconButton,
  Skeleton,
  SkeletonGate,
  StatusDot,
  cx,
  useToast,
  type ComposerHandle,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useAccounts } from "../../stores/accounts";
import { useEngines } from "../../stores/engines";
import { useSession } from "../../stores/session";
import { useTeammates } from "../../stores/teammates";
import { useThreads } from "../../stores/threads";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { LoginSheet, type LoginRequest } from "../settings/AccountsPage";
import { switchMode } from "../shell/actions";
import { pendingApprovals, reuseRows, toRows, usageOf, type Row } from "./rows";
import { useThreadDrops } from "./drops";
import { ThreadComposer } from "./ThreadComposer";
import { ChangesPanel, ChangesToggle } from "./ChangesPanel";
import { columnsFor, GRID_MAX } from "./grid";
import { PlanCard, Transcript, approvalLabels, type TranscriptActions } from "./Transcript";
import { pluginApprovalLabels } from "../plugins/PluginApproval";
import { parseApproval } from "../plugins/model";

const STATE_DOT = {
  idle: "idle",
  working: "working",
  waiting: "waiting",
  done: "done",
  failed: "error",
  interrupted: "idle",
} as const;

function approvalTitle(engine: string, kind: string): string {
  switch (kind) {
    case "command":
      return t("thread.approval.command", { engine });
    case "file_write":
    case "file_delete":
      return t("thread.approval.files", { engine });
    case "network":
      return t("thread.approval.network", { engine });
    case "mcp":
      return t("thread.approval.mcp", { engine });
    default:
      return t("thread.approval.other", { engine });
  }
}

async function resolve(
  approvalId: string,
  decision:
    { type: "allow"; scope: "once" | "thread" | "conversation" } | { type: "deny"; reason: string | null },
) {
  await invoke("approval_resolve", { approvalId, decision, via: "inline" });
}

/** Thread header (UI-20): folder, title, branch, engine and model, state. */
/** "Plan versions (n)": every plan of the thread, newest first (F-TH-10). */
function PlanVersions({ plans, actions }: { plans: PlanRevision[]; actions: TranscriptActions }) {
  const [open, setOpen] = useState(false);
  const [viewing, setViewing] = useState<number | null>(null);
  if (plans.length === 0) return null;
  const steps = (text: string) => text.split("\n").filter((l) => /^\s*(\d+[.)]|[-*])\s+/.test(l)).length;
  return (
    <>
      <Button size="sm" variant="ghost" onClick={() => setOpen(true)} data-testid="plan-versions">
        {t("thread.plan.versions", { n: plans.length })}
      </Button>
      <Dialog open={open} onOpenChange={setOpen} title={t("thread.plan.versionsTitle")} size="lg">
        <ul className="flex flex-col gap-2">
          {[...plans].reverse().map((p) => (
            <li key={p.version} className="flex flex-col gap-2 rounded-md border border-border p-3">
              <div className="flex items-center gap-2 text-sm">
                <span className="font-semibold text-fg">{t("thread.plan", { n: p.version })}</span>
                <span className="text-fg-muted">
                  {new Date(p.createdAt).toLocaleString([], { dateStyle: "medium", timeStyle: "short" })} ·{" "}
                  {t("thread.plan.steps", { n: steps(p.text) })}
                </span>
                <span className="ml-auto flex gap-1">
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => setViewing(viewing === p.version ? null : p.version)}
                  >
                    {t("thread.plan.view")}
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => {
                      setOpen(false);
                      actions.implementPlan(p.version);
                    }}
                  >
                    {t("thread.plan.implementShort")}
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => actions.copy(p.text)}>
                    {t("thread.copy")}
                  </Button>
                </span>
              </div>
              {viewing === p.version && <PlanCard version={p.version} text={p.text} actions={actions} />}
            </li>
          ))}
        </ul>
      </Dialog>
    </>
  );
}

function Header({
  thread,
  contextFill,
  plans,
  actions,
  changes,
  onClose,
}: {
  thread: Thread;
  contextFill: number | null;
  plans: PlanRevision[];
  actions: TranscriptActions;
  changes: ReactNode;
  /** In a grid cell: takes the thread out of the grid. */
  onClose?: () => void;
}) {
  const ws = useWorkspaces((s) => s.list.find((w) => w.id === thread.workspaceId) ?? null);
  const engine = useEngines((s) => s.list.find((e) => e.spec.id === thread.engineId));
  const update = useThreads((s) => s.update);
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState(thread.title);
  const title = thread.title || t("thread.untitled");
  const save = () => {
    setRenaming(false);
    if (name.trim() && name.trim() !== thread.title) void update(thread.id, { title: name.trim() });
  };
  return (
    <header className="flex h-10 shrink-0 items-center gap-2 border-b border-border bg-surface-2 px-3 text-sm">
      <span className="inline-flex shrink-0 items-center gap-1 text-fg-muted">
        <Home size={14} aria-hidden="true" />
        {ws ? ws.name : t("thread.header.noFolder")}
      </span>
      <span aria-hidden="true" className="text-fg-subtle">
        ·
      </span>
      {renaming ? (
        <input
          autoFocus
          aria-label={t("thread.rail.renameField", { name: title })}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onBlur={save}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") setRenaming(false);
          }}
          className="h-7 min-w-0 flex-1 rounded-sm border border-border bg-surface-4 px-2 text-base text-fg focus-visible:border-accent focus-visible:outline-none"
        />
      ) : (
        <button
          type="button"
          data-testid="thread-title"
          onClick={() => {
            setName(thread.title);
            setRenaming(true);
          }}
          title={t("thread.header.rename")}
          className="min-w-0 truncate rounded-sm px-1 text-base font-medium text-fg hover:bg-surface-4 focus-visible:shadow-focus focus-visible:outline-none"
        >
          {title}
        </button>
      )}
      {ws?.branch && (
        <Chip label={ws.branch} icon={<GitBranch size={12} />} className="hidden shrink-0 lg:inline-flex" />
      )}
      <span className="ml-auto flex shrink-0 items-center gap-3">
        <PlanVersions plans={plans} actions={actions} />
        {changes}
        {onClose && (
          <IconButton
            size="sm"
            icon={<X size={14} />}
            label={t("thread.grid.close")}
            onClick={onClose}
            data-testid="grid-close"
          />
        )}
        {engine && <EngineBadge letters={engine.spec.logo} name={engine.spec.label} size={14} />}
        <span className="hidden text-fg-muted md:inline">
          {thread.model ?? t("thread.header.defaultModel")}
        </span>
        <span className="inline-flex items-center gap-1.5 text-fg-muted" data-testid="thread-state">
          <StatusDot status={STATE_DOT[thread.state]} />
          {t(`thread.state.${thread.state}`)}
        </span>
        {contextFill !== null && (
          <ContextMeter
            used={Math.round(contextFill * 10)}
            window={1000}
            label={t("thread.header.context")}
            tooltip={t("thread.header.contextTip", { pct: contextFill })}
          />
        )}
      </span>
    </header>
  );
}

const NO_PLANS: PlanRevision[] = [];

/** Below 1100px the Changes panel floats over the transcript (UI-24). */
function useNarrow(): boolean {
  const query = "(max-width: 1099px)";
  const [narrow, setNarrow] = useState(
    () => typeof window !== "undefined" && !!window.matchMedia?.(query).matches,
  );
  useEffect(() => {
    const m = window.matchMedia?.(query);
    if (!m) return;
    const on = () => setNarrow(m.matches);
    m.addEventListener("change", on);
    return () => m.removeEventListener("change", on);
  }, []);
  return narrow;
}

/** Thread mode with no thread picked: the composer starts one (F-TH-02 AC1). */
function Starter({ visible }: { visible: boolean }) {
  const create = useThreads((s) => s.create);
  const send = useThreads((s) => s.send);
  const engines = useEngines((s) => s.list);
  const loaded = useEngines((s) => s.loaded);
  const { show } = useToast();
  const [text, setText] = useState("");
  const chatEngine = engines.some((e) => e.install && e.chatCapable);
  if (loaded && !chatEngine) {
    return (
      <div className="flex h-full items-center justify-center">
        <EmptyState
          icon={<Bot size={20} />}
          title={t("thread.noEngine.title")}
          body={t("thread.noEngine.body")}
          primary={
            <Button variant="primary" onClick={() => useUi.getState().openSettings("engines")}>
              {t("thread.noEngine.setup")}
            </Button>
          }
          secondary={<Button onClick={() => switchMode("code")}>{t("thread.noEngine.code")}</Button>}
        />
      </div>
    );
  }
  const start = async () => {
    const sent = text;
    setText("");
    const thread = await create(null);
    if ("code" in thread) {
      setText(sent);
      show({ tone: "error", title: thread.message });
      return;
    }
    const res = await send(thread.id, sent, [], false);
    if ("code" in res) show({ tone: "error", title: res.message });
  };
  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-1 items-center justify-center">
        <EmptyState
          icon={<MessagesSquare size={20} />}
          title={t("thread.empty.noFolder.title")}
          body={t("thread.empty.noFolder.body")}
        />
      </div>
      <div className="mx-auto w-full max-w-3xl px-4 pb-4">
        <Composer
          value={text}
          onChange={setText}
          onSend={() => void start()}
          placeholder={t("thread.composer.placeholderNew")}
          labels={{
            message: t("thread.composer.message"),
            send: t("thread.composer.send"),
            emptySend: t("thread.composer.empty"),
          }}
          autoFocus={visible}
        />
      </div>
    </div>
  );
}

const SUGGESTIONS = ["thread.suggest.explain", "thread.suggest.test", "thread.suggest.plan"] as const;

/** UI-20/21/22/23: one thread. */
/** `visible`: Thread mode is showing (every surface stays mounted, F-SH-02). */
/** What a teammate chat changes about the thread view (F-AG-03). */
export type ChatOptions = {
  /** "Message {name}…". */
  placeholder: string;
  /** The empty chat's state (suggestion chips from the brief). */
  empty: (prefill: (text: string) => void) => ReactNode;
  /** Above the composer: the plugin nudge. */
  above?: ReactNode;
  /** Typed `@name` mail, handled before the message is sent; `true` when it
   * was mail and nothing else should go out. */
  beforeSend?: (text: string) => Promise<boolean>;
  /** The thin header (place chip). */
  header?: ReactNode;
};

/** A teammate chat: the same transcript and composer as a thread. */
export function ChatView({
  threadId,
  visible,
  chat,
}: {
  threadId: string;
  visible: boolean;
  chat: ChatOptions;
}) {
  return <ThreadView threadId={threadId} visible={visible} chat={chat} />;
}

function ThreadView({
  threadId,
  visible,
  cell,
  chat,
}: {
  threadId: string;
  visible: boolean;
  cell?: GridCell;
  chat?: ChatOptions;
}) {
  const listed = useThreads((s) => s.list.find((th) => th.id === threadId) ?? null);
  const loaded = useThreads((s) => s.byId[threadId]?.detail?.thread ?? null);
  const chatRow = useTeammates((s) => {
    for (const list of Object.values(s.chats)) {
      const found = list.find((c) => c.id === threadId);
      if (found) return found;
    }
    return null;
  });
  const thread = listed ?? loaded ?? chatRow;
  const live = useThreads((s) => s.byId[threadId]);
  const open = useThreads((s) => s.open);
  const loadOlder = useThreads((s) => s.loadOlder);
  const retry = useThreads((s) => s.retry);
  const implementPlan = useThreads((s) => s.implementPlan);
  const workspace = useWorkspaces((s) => s.list.find((w) => w.id === thread?.workspaceId) ?? null);
  const engineLabel = useEngines((s) => s.label(thread?.engineId) ?? thread?.engineId ?? "");
  const { show } = useToast();
  const composer = useRef<ComposerHandle>(null);
  const [prefill, setPrefill] = useState<{ text: string; n: number } | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [login, setLogin] = useState<LoginRequest | null>(null);

  const previous = useRef<Row[]>([]);
  const rows = useMemo(() => {
    const next = reuseRows(previous.current, live ? toRows(live.view) : []);
    previous.current = next;
    return next;
  }, [live]);
  const usage = useMemo(() => {
    const turns = new Map((live?.detail?.turns ?? []).map((t) => [t.id, t.engineId]));
    return live
      ? usageOf(live.view, (id) => turns.get(id))
      : { input: 0, output: 0, contextFillPct: null, byEngine: [] };
  }, [live]);
  const running = live?.detail?.running ?? null;
  const pending = pendingApprovals(rows, running);

  const fail = useCallback(
    (e: unknown) => show({ tone: "error", title: (e as IpcError)?.message ?? String(e) }),
    [show],
  );
  const actions: TranscriptActions = useMemo(
    () => ({
      approve: (id, scope) => void resolve(id, { type: "allow", scope }).catch(fail),
      deny: (id, reason) => void resolve(id, { type: "deny", reason }).catch(fail),
      retry: () => thread && void retry(thread.id).then((e) => e && fail(e)),
      openTerminal: () => switchMode("code"),
      // The engine's login, in a terminal, for the thread's account (F-TH-06).
      signIn: () => {
        if (!thread) return;
        const mine = useAccounts.getState().list.filter((a) => a.engineId === thread.engineId);
        const account = mine.find((a) => a.id === thread.accountId) ?? mine.find((a) => a.isDefault) ?? null;
        setLogin(
          account
            ? { kind: "session", account, action: "sign_in" }
            : { kind: "add", engineId: thread.engineId },
        );
      },
      editResend: (text) => setPrefill({ text, n: Date.now() }),
      openLink: (href) => void invoke("app_open_external", { url: href }).catch(fail),
      copy: (text) => void navigator.clipboard?.writeText(text),
      implementPlan: (version) => thread && void implementPlan(thread.id, version).then((e) => e && fail(e)),
      revisePlan: () => setPrefill({ text: t("thread.plan.revisePrefill"), n: Date.now() }),
    }),
    [thread, retry, implementPlan, fail],
  );
  const lastMessage = useCallback(() => {
    for (let i = rows.length - 1; i >= 0; i--) {
      const r = rows[i];
      if (r?.kind === "user") return r.text;
    }
    return null;
  }, [rows]);
  // Stable across streamed events, so rows that didn't change don't re-render.
  const turns = live?.detail?.turns;
  const startedAt = useCallback(
    (turnId: string) => turns?.find((tr) => tr.id === turnId)?.startedAt ?? null,
    [turns],
  );
  const approvalLabel = useCallback((kind: string) => approvalTitle(engineLabel, kind), [engineLabel]);
  const plans = live?.detail?.planRevisions;
  const planText = useCallback(
    (version: number) => plans?.find((p) => p.version === version)?.text ?? null,
    [plans],
  );
  const changesOpen = useThreads((s) => !!s.changesOpen[threadId]);
  const toggle = useThreads((s) => s.toggleChanges);
  const toggleChanges = useCallback(() => toggle(threadId), [toggle, threadId]);
  const narrow = useNarrow() || !!cell;
  // A grid cell loads its thread itself (only the open one is selected).
  useEffect(() => {
    if (!useThreads.getState().byId[threadId]) void useThreads.getState().open(threadId);
  }, [threadId]);
  // The Changes panel reads the folder again whenever a turn ends.
  const changesKey = `${live?.detail?.turns.filter((tr) => tr.endedAt !== null).length ?? 0}:${running ?? ""}`;
  const onNearTop = useCallback(() => void loadOlder(threadId), [threadId, loadOlder]);

  if (!thread) return null;

  const loading = !live || live.status === "loading";
  const empty = !loading && live.status === "ready" && rows.length === 0;
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="thread-main" data-thread-id={thread.id}>
      {login && <LoginSheet request={login} onClose={() => setLogin(null)} />}
      {chat ? (
        chat.header
      ) : (
        <Header
          thread={thread}
          contextFill={usage.contextFillPct}
          plans={plans ?? NO_PLANS}
          actions={actions}
          changes={<ChangesToggle open={changesOpen} onToggle={toggleChanges} />}
          onClose={cell?.onClose}
        />
      )}
      <div className="relative flex min-h-0 flex-1">
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          {live?.status === "error" ? (
            <div className="p-4">
              <Banner
                tone="error"
                action={{ label: t("thread.reload"), onClick: () => void open(thread.id) }}
              >
                {live.error?.message ?? t("thread.loadFailed")}
              </Banner>
            </div>
          ) : (
            <SkeletonGate
              loading={loading}
              label={t("thread.loading")}
              skeleton={
                <div className="mx-auto flex w-full max-w-3xl flex-col gap-4 p-4">
                  {[0, 1, 2, 3].map((n) => (
                    <Skeleton
                      key={n}
                      height={n % 2 ? 56 : 32}
                      width={n % 2 ? "100%" : "60%"}
                      className={n % 2 ? "" : "self-end"}
                    />
                  ))}
                </div>
              }
            >
              {empty && chat ? (
                <div className="flex min-h-0 flex-1 items-center justify-center">
                  {chat.empty((text) => setPrefill({ text, n: Date.now() }))}
                </div>
              ) : empty ? (
                <div className="flex min-h-0 flex-1 items-center justify-center">
                  {workspace ? (
                    <EmptyState
                      icon={<MessagesSquare size={20} />}
                      title={t("thread.empty.folder.title", { folder: workspace.name })}
                      body={t("thread.empty.folder.body")}
                      primary={
                        <span className="flex flex-wrap justify-center gap-2">
                          {SUGGESTIONS.map((k) => (
                            <Button
                              key={k}
                              size="sm"
                              variant="secondary"
                              onClick={() => setPrefill({ text: t(k), n: Date.now() })}
                            >
                              {t(k)}
                            </Button>
                          ))}
                        </span>
                      }
                    />
                  ) : (
                    <EmptyState
                      icon={<FolderOpen size={20} />}
                      title={t("thread.empty.noFolder.title")}
                      body={t("thread.empty.noFolder.body")}
                      primary={<AddFolder thread={thread} />}
                    />
                  )}
                </div>
              ) : (
                <Transcript
                  rows={rows}
                  actions={actions}
                  approvalLabel={approvalLabel}
                  startedAt={startedAt}
                  planText={planText}
                  onNearTop={onNearTop}
                />
              )}
            </SkeletonGate>
          )}
          <div className="mx-auto flex w-full max-w-3xl shrink-0 flex-col gap-2 px-4 pb-4">
            {thread.state === "interrupted" && running === null && (
              // A crash or quit stopped its turn: never shown as done (F-SH-07).
              <div data-testid="thread-interrupted">
                <Banner
                  tone="warning"
                  className="rounded-md"
                  action={{ label: t("thread.interrupted.resume"), onClick: actions.retry }}
                >
                  {t("thread.interrupted.title")}
                </Banner>
              </div>
            )}
            {pending.length > 0 && (
              <div className="flex flex-col gap-2" data-testid="thread-pending">
                {(expanded ? pending : pending.slice(0, 1)).map((p) => {
                  // A plugin's card answers "for this chat" (F-PL-07).
                  const plugin = parseApproval(p.request.detail);
                  const wide = plugin ? "conversation" : "thread";
                  return (
                    <ApprovalCard
                      key={p.request.id}
                      compact
                      kind={p.request.kind}
                      title={`${approvalTitle(engineLabel, p.request.kind)}: ${p.request.summary}`}
                      allowThread={p.request.options.includes(wide)}
                      onAllow={() => actions.approve(p.request.id, "once")}
                      onAllowThread={() => actions.approve(p.request.id, wide)}
                      onDeny={(reason) => actions.deny(p.request.id, reason)}
                      labels={{
                        ...approvalLabels(p.request.kind),
                        ...(plugin ? pluginApprovalLabels() : {}),
                      }}
                    />
                  );
                })}
                {pending.length > 1 && !expanded && (
                  <Button size="sm" variant="ghost" className="self-start" onClick={() => setExpanded(true)}>
                    {t("thread.approval.more", { n: pending.length - 1 })}
                  </Button>
                )}
              </div>
            )}
            {chat?.above}
            <ThreadComposer
              key={thread.id}
              thread={thread}
              chat={chat ? { placeholder: chat.placeholder, beforeSend: chat.beforeSend } : undefined}
              detail={live?.detail ?? null}
              running={running !== null}
              tokens={usage}
              folderName={workspace?.name ?? null}
              lastMessage={lastMessage}
              composerRef={composer}
              prefill={prefill}
              visible={visible && (!cell || cell.selected)}
            />
          </div>
        </div>
        {changesOpen && (
          <ChangesPanel
            threadId={thread.id}
            refreshKey={changesKey}
            rows={rows}
            floating={narrow}
            onClose={toggleChanges}
          />
        )}
      </div>
    </div>
  );
}

type GridCell = { selected: boolean; onClose: () => void };

const THREAD_MIME = "application/x-vibezeo-thread";

/** Rail rows carry their thread id when dragged (F-TH-14). */
export function dragThread(e: DragEvent, id: string): void {
  e.dataTransfer.setData(THREAD_MIME, id);
  e.dataTransfer.effectAllowed = "copy";
}

/** Adds a thread to the grid and says why when it can't (F-TH-14 AC1). */
export function useAddToGrid(): (id: string) => void {
  const addToGrid = useThreads((s) => s.addToGrid);
  const openUpgrade = useUi((s) => s.openUpgrade);
  const { show } = useToast();
  return useCallback(
    (id: string) => {
      const result = addToGrid(id);
      if (result === "full") show({ tone: "warning", title: t("thread.grid.full", { n: GRID_MAX }) });
      if (result === "limit") {
        const limit = useSession.getState().entitlement?.limits.threadGrid ?? 1;
        show({
          tone: "info",
          title: t("thread.grid.freeTitle"),
          body: t("thread.grid.freeBody"),
          action: {
            label: t("thread.grid.plans"),
            onClick: () => openUpgrade({ kind: "thread_grid", limit }),
          },
        });
      }
    },
    [addToGrid, openUpgrade, show],
  );
}

/** The thread area takes threads dragged from the rail. */
function useGridDrop(): {
  over: boolean;
  handlers: Pick<HTMLAttributes<HTMLDivElement>, "onDragOver" | "onDragLeave" | "onDrop">;
} {
  const add = useAddToGrid();
  const [over, setOver] = useState(false);
  const carries = (e: DragEvent) => Array.from(e.dataTransfer.types).includes(THREAD_MIME);
  return {
    over,
    handlers: {
      onDragOver: (e) => {
        if (!carries(e)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
        setOver(true);
      },
      onDragLeave: (e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setOver(false);
      },
      onDrop: (e) => {
        if (!carries(e)) return;
        e.preventDefault();
        setOver(false);
        const id = e.dataTransfer.getData(THREAD_MIME);
        if (id) add(id);
      },
    },
  };
}

/** F-TH-14: up to six threads side by side, 1–3 columns by width. */
function ThreadGrid({ visible, grid }: { visible: boolean; grid: string[] }) {
  const selectedId = useThreads((s) => s.selectedId);
  const select = useThreads((s) => s.select);
  const remove = useThreads((s) => s.removeFromGrid);
  const setColumns = useThreads((s) => s.setGridColumns);
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(1200);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    setWidth(el.clientWidth || 1200);
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth || 1200));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const columns = columnsFor(width, grid.length);
  useEffect(() => setColumns(columns), [columns, setColumns]);
  return (
    <div
      ref={ref}
      data-testid="thread-grid"
      className={cx("grid h-full min-h-0 auto-rows-fr gap-px bg-border", COLUMNS[columns] ?? "grid-cols-3")}
    >
      {grid.map((id) => (
        <div
          key={id}
          data-testid="grid-cell"
          data-thread-id={id}
          aria-current={id === selectedId ? "true" : undefined}
          className={cx(
            "relative min-h-0 min-w-0 bg-surface-1",
            id === selectedId && "ring-1 ring-inset ring-accent",
          )}
          onPointerDownCapture={() => id !== selectedId && select(id)}
          onFocusCapture={() => id !== useThreads.getState().selectedId && select(id)}
        >
          <ThreadView
            threadId={id}
            visible={visible}
            cell={{ selected: id === selectedId, onClose: () => remove(id) }}
          />
        </div>
      ))}
    </div>
  );
}

const COLUMNS: Record<number, string> = { 1: "grid-cols-1", 2: "grid-cols-2", 3: "grid-cols-3" };

/** UI-20/21/22/23: Thread mode's main area: one thread, a grid of them
 * (F-TH-14), or the starter. `visible`: Thread mode is showing (every
 * surface stays mounted, F-SH-02). */
export function ThreadMain({ visible = true }: { visible?: boolean }) {
  const selectedId = useThreads((s) => s.selectedId);
  const exists = useThreads((s) => s.list.some((th) => th.id === s.selectedId));
  const saved = useThreads((s) => s.grid);
  // The plan decides how many show; a saved grid outlives a downgrade.
  const limit = useSession((s) => s.entitlement?.limits.threadGrid ?? 1);
  const grid = useMemo(() => (limit > 1 ? saved.slice(0, Math.min(limit, GRID_MAX)) : []), [saved, limit]);
  const { show } = useToast();
  useThreadDrops(show);
  const { over, handlers } = useGridDrop();
  return (
    <div className="relative h-full min-h-0" data-testid="thread-area" {...handlers}>
      {grid.length > 1 ? (
        <ThreadGrid visible={visible} grid={grid} />
      ) : selectedId && exists ? (
        <ThreadView threadId={selectedId} visible={visible} />
      ) : (
        <Starter visible={visible} />
      )}
      {over && (
        <div className="pointer-events-none absolute inset-2 z-30 flex items-center justify-center rounded-md border-2 border-dashed border-accent bg-surface-2/80 text-md text-fg">
          {t("thread.grid.drop")}
        </div>
      )}
    </div>
  );
}

/** "Add folder…" for a folderless thread (F-TH-02): the draft stays. */
function AddFolder({ thread }: { thread: Thread }) {
  const add = useWorkspaces((s) => s.add);
  const reloadDetail = useThreads((s) => s.reloadDetail);
  const { show } = useToast();
  return (
    <Button
      variant="primary"
      onClick={async () => {
        const root = await invoke<string | null>("workspace_pick_folder").catch(() => null);
        if (!root) return;
        const res = await add(root);
        if (!res.ok) {
          show({ tone: "error", title: res.error.message });
          return;
        }
        await invoke("thread_move_to_folder", {
          threadId: thread.id,
          workspaceId: res.added.workspace.id,
        }).catch((e: IpcError) => show({ tone: "error", title: e.message }));
        void reloadDetail(thread.id);
      }}
    >
      {t("thread.addFolder")}
    </Button>
  );
}
