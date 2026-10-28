// The sidebar's Checkpoints tab (F-EX-01, UI-140): the folder's checkpoints
// for the context on screen (a thread's own, or a Code workspace's or lane's),
// each against now, with a diff per file and restore for all or chosen files.
import { useCallback, useEffect, useMemo, useState } from "react";
import { History, RefreshCw } from "lucide-react";
import type { Checkpoint, CheckpointTimeline, FileContext, FileDiff, IpcError, RestoreReport } from "@tandem/protocol";
import {
  Badge,
  Banner,
  Button,
  Callout,
  Checkbox,
  ConfirmDialog,
  DiffView,
  EmptyState,
  IconButton,
  ListRow,
  Skeleton,
  Spinner,
  cx,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { useCheckpointFocus } from "../../stores/activity";
import { useLayout } from "../../stores/layout";
import { useSettings } from "../../stores/settings";
import { paneKey, useStatus } from "../../stores/status";
import { useThreads } from "../../stores/threads";
import { age } from "../thread/groups";
import { diffLabels } from "../files/ChangesView";
import { useActiveFiles } from "../files/context";

type Load<T> = { at: "loading" } | { at: "error"; message: string } | { at: "ready"; value: T };

/** "Before a turn", "Before a prompt", "Checkpoint", "Before restore". */
export function reasonLabel(c: Checkpoint): string {
  switch (c.reason) {
    case "turn":
      return t("checkpoints.reason.turn");
    case "prompt":
      return t("checkpoints.reason.prompt");
    case "restore":
      return t("checkpoints.reason.restore");
    default:
      return t("checkpoints.reason.manual");
  }
}

function changeLabel(d: FileDiff): string {
  if (d.change === "added") return t("checkpoints.change.created");
  if (d.change === "deleted") return t("checkpoints.change.deleted");
  if (d.binary) return t("checkpoints.change.binary");
  return t("checkpoints.change.lines", { add: d.additions, del: d.deletions });
}

function message(e: unknown): string {
  return (e as IpcError)?.message ?? String(e);
}

/** Agents working in `workspaceId` now: its agent panes and threads. */
function useWorkingHere(workspaceId: string | null): string[] {
  const records = useStatus((s) => s.records);
  const tabs = useLayout((s) => s.tabs);
  const threads = useThreads((s) => s.list);
  return useMemo(() => {
    if (!workspaceId) return [];
    const names: string[] = [];
    for (const tab of Object.values(tabs)) {
      for (const p of Object.values(tab.panes)) {
        if (p.workspaceId === workspaceId && p.kind === "agent" && records[paneKey(p.id)]?.status === "working")
          names.push(p.humanName ?? p.engineId ?? "");
      }
    }
    for (const th of threads) if (th.workspaceId === workspaceId && th.state === "working") names.push(th.title);
    return names.filter(Boolean);
  }, [workspaceId, records, tabs, threads]);
}

function Row({ c, selected, onSelect, now }: { c: Checkpoint; selected: boolean; onSelect: () => void; now: number }) {
  const secondary = [c.label, age(c.createdAt, now)].filter(Boolean).join(" · ");
  return (
    <ListRow
      role="option"
      aria-selected={selected}
      tabIndex={selected ? 0 : -1}
      data-testid="checkpoint-row"
      data-id={c.id}
      data-reason={c.reason}
      data-differs={c.differs}
      selected={selected}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect();
        }
      }}
      leading={<History size={14} aria-hidden="true" className="text-fg-subtle" />}
      title={reasonLabel(c)}
      secondary={secondary}
      trailing={
        <span className="flex items-center gap-1">
          {c.late && <Badge variant="waiting">{t("checkpoints.late")}</Badge>}
          {c.partial && <Badge variant="outline">{t("checkpoints.partial")}</Badge>}
          <Badge variant={c.differs ? "neutral" : "outline"}>
            {c.differs ? t("checkpoints.differs", { n: c.differs }) : t("checkpoints.same")}
          </Badge>
        </span>
      }
    />
  );
}

type Ask = { checkpoint: Checkpoint; paths: string[] | null; n: number } | null;

function Detail({
  checkpoint,
  diff,
  reload,
  onRestore,
  working,
}: {
  checkpoint: Checkpoint;
  diff: Load<FileDiff[]>;
  reload: () => void;
  onRestore: (paths: string[] | null, n: number) => void;
  working: boolean;
}) {
  const files = diff.at === "ready" ? diff.value : [];
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [focus, setFocus] = useState<string | null>(null);
  useEffect(() => {
    setChecked(new Set(files.map((f) => f.path)));
    setFocus(files[0]?.path ?? null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [diff]);
  if (diff.at === "loading")
    return (
      <div className="flex items-center gap-2 p-3 text-sm text-fg-muted">
        <Spinner size={12} /> {t("checkpoints.diff.loading")}
      </div>
    );
  if (diff.at === "error")
    return (
      <div className="p-3">
        <Banner tone="error" action={{ label: t("checkpoints.retry"), onClick: reload }}>
          {t("checkpoints.diff.failed", { reason: diff.message })}
        </Banner>
      </div>
    );
  if (!files.length)
    return <p className="p-3 text-sm text-fg-muted" data-testid="checkpoint-same">{t("checkpoints.diff.same")}</p>;
  const shown = files.find((f) => f.path === focus) ?? files[0];
  const picked = files.filter((f) => checked.has(f.path)).map((f) => f.path);
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="checkpoint-detail" data-id={checkpoint.id}>
      <ul className="max-h-48 shrink-0 overflow-auto border-b border-divider py-1" aria-label={t("checkpoints.files")}>
        {files.map((f) => (
          <li
            key={f.path}
            data-testid="checkpoint-file"
            data-path={f.path}
            data-change={f.change}
            className={cx("flex items-center gap-2 px-2 py-0.5 text-sm", f.path === shown?.path && "bg-surface-2")}
          >
            <Checkbox
              label={<span className="sr-only">{t("checkpoints.pick", { path: f.path })}</span>}
              checked={checked.has(f.path)}
              onCheckedChange={(on) =>
                setChecked((prev) => {
                  const next = new Set(prev);
                  if (on) next.add(f.path);
                  else next.delete(f.path);
                  return next;
                })
              }
            />
            <button
              type="button"
              onClick={() => setFocus(f.path)}
              className="min-w-0 flex-1 truncate text-left font-mono text-xs text-fg hover:underline focus-visible:shadow-focus focus-visible:outline-none"
            >
              {f.path}
            </button>
            <span className="shrink-0 text-xs text-fg-subtle tabular-nums">{changeLabel(f)}</span>
          </li>
        ))}
      </ul>
      <div className="min-h-0 flex-1 overflow-auto p-2">{shown && <DiffView files={[shown]} labels={diffLabels()} />}</div>
      <div className="flex shrink-0 justify-end gap-2 border-t border-divider p-2">
        <Button
          variant="secondary"
          size="sm"
          disabled={working || picked.length === 0 || picked.length === files.length}
          data-testid="checkpoint-restore-some"
          onClick={() => onRestore(picked, picked.length)}
        >
          {t("checkpoints.restoreSome", { n: picked.length })}
        </Button>
        <Button variant="primary" size="sm" loading={working} data-testid="checkpoint-restore-all" onClick={() => onRestore(null, files.length)}>
          {t("checkpoints.restoreAll")}
        </Button>
      </div>
    </div>
  );
}

export function CheckpointsPanel() {
  const active = useActiveFiles();
  // F-EX-07: an Activity link opens on its checkpoint's own folder.
  const focus = useCheckpointFocus((s) => s.focus);
  useEffect(() => () => useCheckpointFocus.getState().set(null), []);
  const context: FileContext | null = focus
    ? { type: "workspace", workspaceId: focus.workspaceId }
    : active.kind === "context"
      ? active.context
      : null;
  const key = context ? JSON.stringify(context) : "";
  const [timeline, setTimeline] = useState<Load<CheckpointTimeline>>({ at: "loading" });
  const [selected, setSelected] = useState<string | null>(null);
  const [diff, setDiff] = useState<Load<FileDiff[]>>({ at: "loading" });
  const [ask, setAsk] = useState<Ask>(null);
  const [working, setWorking] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const { show } = useToast();
  const workingHere = useWorkingHere(active.kind === "context" ? active.workspaceId : null);

  const load = useCallback(async () => {
    if (!context) return;
    setNow(Date.now());
    try {
      const value = await invoke<CheckpointTimeline>("checkpoints_list", { context }, { timeoutMs: 60_000 });
      setTimeline({ at: "ready", value });
    } catch (e) {
      setTimeline({ at: "error", message: message(e) });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const loadDiff = useCallback(async (id: string) => {
    setDiff({ at: "loading" });
    try {
      setDiff({ at: "ready", value: await invoke<FileDiff[]>("checkpoint_diff", { checkpointId: id }, { timeoutMs: 60_000 }) });
    } catch (e) {
      setDiff({ at: "error", message: message(e) });
    }
  }, []);

  useEffect(() => {
    setTimeline({ at: "loading" });
    setSelected(null);
    void load();
  }, [load]);

  // ...with that checkpoint selected, once the list is in.
  useEffect(() => {
    if (focus && timeline.at === "ready") setSelected(focus.checkpointId);
  }, [focus, timeline.at]);

  // A new checkpoint for this folder: the list again (the diff stays).
  const folder = timeline.at === "ready" ? timeline.value.folder : null;
  useEffect(() => {
    if (!folder) return;
    let live = true;
    let off: (() => void) | undefined;
    void listen<{ folder: string }>("checkpoint:created", (p) => {
      if (live && p.folder === folder) void load();
    }).then((u) => {
      if (live) off = u;
      else u();
    });
    return () => {
      live = false;
      off?.();
    };
  }, [folder, load]);

  useEffect(() => {
    if (selected) void loadDiff(selected);
  }, [selected, loadDiff]);

  const items = useMemo(() => (timeline.at === "ready" ? timeline.value.items : []), [timeline]);
  const current = useMemo(() => items.find((c) => c.id === selected) ?? null, [items, selected]);

  const checkpointNow = async () => {
    if (!context) return;
    try {
      const made = await invoke<Checkpoint | null>("checkpoint_create", { context }, { timeoutMs: 60_000 });
      show(made ? { tone: "success", title: t("checkpoints.saved") } : { tone: "info", title: t("checkpoints.unchanged") });
      await load();
    } catch (e) {
      show({ tone: "error", title: message(e) });
    }
  };

  const restore = async (target: NonNullable<Ask>) => {
    setWorking(true);
    try {
      const report = await invoke<RestoreReport>(
        "checkpoint_restore",
        { checkpointId: target.checkpoint.id, paths: target.paths },
        { timeoutMs: 120_000 },
      );
      const undo = report.undoId;
      if (report.failed.length) {
        show({
          tone: "error",
          title: t("checkpoints.restoreFailed", { n: report.failed.length }),
          body: report.failed.map((f) => `${f.path}: ${f.message}`).join("\n"),
        });
      } else {
        show({
          tone: "success",
          title: t("checkpoints.restored", { n: report.restored.length }),
          action: undo
            ? {
                label: t("checkpoints.undo"),
                onClick: () =>
                  void invoke<RestoreReport>("checkpoint_restore", { checkpointId: undo, paths: null }, { timeoutMs: 120_000 }).then(
                    () => void load(),
                    (e) => show({ tone: "error", title: message(e) }),
                  ),
              }
            : undefined,
        });
      }
      await load();
      await loadDiff(target.checkpoint.id);
    } catch (e) {
      show({ tone: "error", title: message(e) });
    } finally {
      setWorking(false);
    }
  };

  if (active.kind === "none")
    return <EmptyState icon={<History size={18} />} title={t("checkpoints.noContext")} body={t("checkpoints.noContext.body")} />;
  if (active.kind === "no_folder")
    return <EmptyState icon={<History size={18} />} title={t("checkpoints.noFolder")} body={t("checkpoints.noFolder.body")} />;

  const header = (
    <div className="flex h-9 shrink-0 items-center gap-2 border-b border-divider px-2">
      <span className="min-w-0 flex-1 truncate text-sm font-medium text-fg">{t("checkpoints.title", { name: active.name })}</span>
      <IconButton size="sm" data-testid="checkpoints-refresh" icon={<RefreshCw size={13} />} label={t("checkpoints.refresh")} onClick={() => void load()} />
      <Button
        size="sm"
        variant="secondary"
        data-testid="checkpoint-now"
        disabled={timeline.at !== "ready" || !timeline.value.enabled || timeline.value.store === "no_git"}
        onClick={() => void checkpointNow()}
      >
        {t("checkpoints.now")}
      </Button>
    </div>
  );

  let body;
  if (timeline.at === "loading") {
    body = (
      <div className="space-y-2 p-3" data-testid="checkpoints-loading">
        <Skeleton className="h-8" />
        <Skeleton className="h-8" />
        <Skeleton className="h-8" />
      </div>
    );
  } else if (timeline.at === "error") {
    body = (
      <div className="p-3">
        <Banner tone="error" action={{ label: t("checkpoints.retry"), onClick: () => void load() }}>
          {t("checkpoints.failed", { reason: timeline.message })}
        </Banner>
      </div>
    );
  } else if (timeline.value.store === "no_git") {
    body = <EmptyState icon={<History size={18} />} title={t("checkpoints.noGit")} />;
  } else if (!timeline.value.enabled && items.length === 0) {
    body = (
      <EmptyState
        icon={<History size={18} />}
        title={t("checkpoints.off")}
        primary={
          <Button variant="primary" onClick={() => void useSettings.getState().set("checkpoints_enabled", true).then(() => void load())}>
            {t("checkpoints.turnOn")}
          </Button>
        }
      />
    );
  } else {
    body = (
      <div className="flex min-h-0 flex-1 flex-col">
        {timeline.value.store === "folder" && (
          <div className="shrink-0 p-2" data-testid="checkpoints-folder-note">
            <Callout tone="info">{t("checkpoints.folderNote")}</Callout>
          </div>
        )}
        {!timeline.value.enabled && (
          <div className="shrink-0 p-2">
            <Callout tone="warning">{t("checkpoints.offNote")}</Callout>
          </div>
        )}
        {items.length === 0 ? (
          <EmptyState icon={<History size={18} />} title={t("checkpoints.empty")} body={t("checkpoints.empty.body")} />
        ) : (
          <>
            <div role="listbox" aria-label={t("checkpoints.list")} className={cx("overflow-auto py-1", current ? "max-h-56 shrink-0 border-b border-divider" : "flex-1")}>
              {items.map((c) => (
                <Row key={c.id} c={c} now={now} selected={c.id === selected} onSelect={() => setSelected(c.id)} />
              ))}
            </div>
            {current && (
              <Detail
                checkpoint={current}
                diff={diff}
                working={working}
                reload={() => void loadDiff(current.id)}
                onRestore={(paths, n) => setAsk({ checkpoint: current, paths, n })}
              />
            )}
          </>
        )}
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="checkpoints-panel" data-store={timeline.at === "ready" ? timeline.value.store : undefined}>
      {header}
      {body}
      <ConfirmDialog
        open={ask !== null}
        onOpenChange={(o) => !o && setAsk(null)}
        danger
        title={ask ? t("checkpoints.confirm.title", { n: ask.n, name: reasonLabel(ask.checkpoint) }) : ""}
        body={
          <>
            <p>{t("checkpoints.confirm.body")}</p>
            {workingHere.length > 0 && (
              <p className="mt-2" data-testid="checkpoint-confirm-working">
                {t("checkpoints.confirm.working", { name: workingHere[0] ?? "", n: workingHere.length })}
              </p>
            )}
          </>
        }
        confirmLabel={t("checkpoints.confirm.action")}
        onConfirm={() => {
          const target = ask;
          setAsk(null);
          if (target) void restore(target);
        }}
      />
    </div>
  );
}

