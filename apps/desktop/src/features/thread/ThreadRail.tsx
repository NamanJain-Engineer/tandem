import { useEffect, useMemo, useRef, useState } from "react";
import { Archive, MessagesSquare, Plus, Search } from "lucide-react";
import type { Thread, ThreadState } from "@tandem/protocol";
import {
  Button,
  ConfirmDialog,
  ContextMenu,
  EngineGlyph,
  IconButton,
  Input,
  StatusDot,
  orderEntries,
  useToast,
  type MenuEntry,
  type Status,
} from "@tandem/ui";
import { dragThread, useAddToGrid } from "./ThreadMain";
import { t } from "../../i18n";
import { useEngines } from "../../stores/engines";
import { useThreads } from "../../stores/threads";
import { useWorkspaces } from "../../stores/workspaces";
import { age, groupThreads, type RailGroup } from "./groups";

const STATUS: Record<ThreadState, Status> = {
  idle: "idle",
  working: "working",
  waiting: "waiting",
  done: "done",
  failed: "error",
  interrupted: "idle",
};

function rightLabel(thread: Thread, now: number): string {
  switch (thread.state) {
    case "working":
      return t("thread.rail.working");
    case "waiting":
      return t("thread.rail.waiting");
    case "failed":
      return t("thread.rail.failed");
    default:
      return age(thread.updatedAt, now);
  }
}

function groupLabel(g: RailGroup): string {
  if (g.kind === "pinned") return t("thread.rail.pinned");
  if (g.kind === "none") return t("thread.rail.noFolder");
  if (g.kind === "other") return t("thread.rail.other");
  return g.name ?? "";
}

function Row({ thread, selected, now }: { thread: Thread; selected: boolean; now: number }) {
  const select = useThreads((s) => s.select);
  const update = useThreads((s) => s.update);
  const archive = useThreads((s) => s.archive);
  const remove = useThreads((s) => s.remove);
  const glyph = useEngines((s) => s.glyph(thread.engineId));
  const engine = useEngines((s) => s.label(thread.engineId));
  const { show } = useToast();
  const addToGrid = useAddToGrid();
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState(thread.title);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const title = thread.title || t("thread.untitled");

  const entries: MenuEntry[] = orderEntries([
    { id: "rename", label: t("thread.rail.rename"), kbd: ["F2"], onSelect: () => setRenaming(true) },
    {
      id: "pin",
      label: thread.pinned ? t("thread.rail.unpin") : t("thread.rail.pin"),
      disabled: thread.archived,
      onSelect: () => void update(thread.id, { pinned: !thread.pinned }),
    },
    {
      id: "grid",
      label: t("thread.grid.add"),
      disabled: thread.archived,
      onSelect: () => addToGrid(thread.id),
    },
    {
      id: "archive",
      label: thread.archived ? t("thread.rail.restore") : t("thread.rail.archive"),
      onSelect: () =>
        void archive(thread.id, !thread.archived).then((e) => e && show({ tone: "error", title: e.message })),
    },
    ...(thread.archived
      ? [
          {
            id: "delete",
            label: t("thread.rail.delete"),
            danger: true,
            onSelect: () => setConfirmDelete(true),
          },
        ]
      : []),
  ]);

  const save = () => {
    setRenaming(false);
    const next = name.trim();
    if (next && next !== thread.title) void update(thread.id, { title: next });
    else setName(thread.title);
  };

  if (renaming) {
    return (
      <li className="px-1">
        <Input
          label={t("thread.rail.renameField", { name: title })}
          hideLabel
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          onBlur={save}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") {
              setName(thread.title);
              setRenaming(false);
            }
          }}
        />
      </li>
    );
  }

  return (
    <li>
      <ContextMenu entries={entries}>
        <button
          type="button"
          data-testid="thread-row"
          data-thread-id={thread.id}
          draggable={!thread.archived}
          onDragStart={(e) => dragThread(e, thread.id)}
          aria-current={selected ? "page" : undefined}
          onClick={() => select(thread.id)}
          onKeyDown={(e) => {
            if (e.key === "F2") {
              e.preventDefault();
              setRenaming(true);
            }
          }}
          title={title}
          className={
            "group flex h-[30px] w-full select-none items-center gap-1.5 rounded-sm px-2 text-left text-base text-fg outline-none hover:bg-surface-4 focus-visible:shadow-focus " +
            (selected ? "bg-accent-soft hover:bg-accent-soft" : "")
          }
        >
          <StatusDot status={STATUS[thread.state]} label={t(`thread.state.${thread.state}`)} />
          <span className={"min-w-0 flex-1 truncate " + (thread.archived ? "text-fg-muted" : "")}>
            {title}
          </span>
          {thread.unread && (
            <span
              aria-label={t("thread.rail.unread")}
              role="img"
              className="size-1.5 shrink-0 rounded-full bg-accent"
            />
          )}
          <span className="shrink-0 text-sm text-fg-subtle">{rightLabel(thread, now)}</span>
          <EngineGlyph letters={glyph} size={14} label={engine ?? undefined} />
        </button>
      </ContextMenu>
      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        danger
        title={t("thread.delete.title", { name: title })}
        body={t("thread.delete.body")}
        confirmLabel={t("thread.delete.ok")}
        onConfirm={() => {
          setConfirmDelete(false);
          void remove(thread.id).then((e) => e && show({ tone: "error", title: e.message }));
        }}
      />
    </li>
  );
}

/** UI-20 rail: Pinned, No folder, a group per folder, Other. */
export function ThreadRail({ collapsed }: { collapsed: boolean }) {
  const list = useThreads((s) => s.list);
  const loaded = useThreads((s) => s.loaded);
  const selectedId = useThreads((s) => s.selectedId);
  const showArchived = useThreads((s) => s.showArchived);
  const setShowArchived = useThreads((s) => s.setShowArchived);
  const create = useThreads((s) => s.create);
  const workspaces = useWorkspaces((s) => s.list);
  const [query, setQuery] = useState("");
  const [now, setNow] = useState(() => Date.now());
  const searchRef = useRef<HTMLInputElement>(null);
  const { show } = useToast();

  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(id);
  }, []);

  const groups = useMemo(
    () => groupThreads(list, workspaces, { showArchived, query }),
    [list, workspaces, showArchived, query],
  );
  const newThread = (workspaceId: string | null) =>
    void create(workspaceId).then((r) => {
      if ("code" in r) show({ tone: "error", title: r.message });
    });

  if (collapsed) {
    return (
      <nav
        aria-label={t("rail.threads")}
        className="flex w-[var(--rail-collapsed)] shrink-0 flex-col items-center gap-1 border-r border-border bg-surface-1 py-2"
      >
        <IconButton
          icon={<Plus size={16} />}
          label={t("thread.rail.new")}
          kbd={["Mod", "N"]}
          onClick={() => newThread(null)}
        />
      </nav>
    );
  }

  return (
    <nav
      aria-label={t("rail.threads")}
      className="flex w-[var(--rail-w)] shrink-0 flex-col border-r border-border bg-surface-1"
    >
      <header className="flex h-9 items-center justify-between pl-3 pr-1.5">
        <h2 className="text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
          {t("rail.threads").toUpperCase()}
        </h2>
        <IconButton
          size="sm"
          icon={<Plus size={16} />}
          label={t("thread.rail.new")}
          kbd={["Mod", "N"]}
          onClick={() => newThread(null)}
          data-testid="thread-new"
        />
      </header>
      <div className="px-2 pb-2">
        <Input
          ref={searchRef}
          label={t("thread.rail.filter")}
          hideLabel
          placeholder={t("thread.rail.filter")}
          prefix={<Search size={14} />}
          clearable
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onClear={() => setQuery("")}
        />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2">
        {loaded && list.filter((th) => th.kind === "thread").length === 0 ? (
          <div className="flex flex-col items-center gap-2 px-3 py-6 text-center">
            <MessagesSquare size={20} aria-hidden="true" className="text-fg-subtle" />
            <p className="text-sm text-fg-muted">{t("thread.rail.empty")}</p>
            <Button size="sm" variant="secondary" onClick={() => newThread(null)}>
              {t("thread.rail.new")}
            </Button>
          </div>
        ) : groups.length === 0 && query ? (
          <p className="px-2 py-4 text-sm text-fg-muted">{t("thread.rail.noMatch")}</p>
        ) : (
          groups.map((g) => (
            <section key={g.id} aria-label={groupLabel(g)} className="pt-2">
              <div className="group flex h-6 items-center justify-between px-2">
                <h3 className="truncate text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
                  {groupLabel(g).toUpperCase()}
                </h3>
                {g.kind === "folder" && (
                  <IconButton
                    size="sm"
                    className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                    icon={<Plus size={14} />}
                    label={t("thread.rail.newIn", { folder: g.name ?? "" })}
                    onClick={() => newThread(g.workspaceId)}
                  />
                )}
              </div>
              <ul className="flex flex-col">
                {g.threads.map((th) => (
                  <Row key={th.id} thread={th} selected={th.id === selectedId} now={now} />
                ))}
              </ul>
            </section>
          ))
        )}
      </div>
      <footer className="border-t border-divider px-2 py-1.5">
        <Button
          size="sm"
          variant="ghost"
          iconLeft={<Archive size={14} />}
          aria-pressed={showArchived}
          onClick={() => setShowArchived(!showArchived)}
        >
          {showArchived ? t("thread.rail.hideArchived") : t("thread.rail.showArchived")}
        </Button>
      </footer>
    </nav>
  );
}
