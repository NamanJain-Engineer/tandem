import { useEffect, useRef, useState } from "react";
import { AlertTriangle, GitBranch, MoreHorizontal } from "lucide-react";
import type { FileDiff, MergeReport, WorkspaceSummary } from "@tandem/protocol";
import {
  Badge,
  Button,
  ConfirmDialog,
  Dialog,
  IconButton,
  Input,
  Menu,
  Skeleton,
  Tooltip,
  cx,
  useToast,
  type MenuEntry,
} from "@tandem/ui";
import { t } from "../../i18n";
import { useSidebar } from "../../stores/sidebar";
import { useWorkspaces } from "../../stores/workspaces";
import {
  baseBranch,
  createPullRequest,
  createWorktree,
  PREVIEW_POLL_MS,
  loadForge,
  loadRuntime,
  openPreview,
  pollPreviews,
  startPreview,
  mergeWorktree,
  prepareWorktree,
  removeWorktree,
  teardownText,
  useWorktrees,
  worktreeDiff,
} from "../../stores/worktrees";
import { switchMode } from "../shell/actions";
import { ComparePreviewsDialog } from "./ComparePreviews";
import { MergeForecast, OverlapDialog } from "./WorktreeOverlap";
import { SecondOpinionDialog } from "./SecondOpinion";
import { ReviewDiff } from "./WorktreeReview";
import { InstructionsDialog } from "./Instructions";
import { ReviewSetupDialog, WorktreeSetupDialog, useWorktreeNotices } from "./WorktreeSetup";

type Show = ReturnType<typeof useToast>["show"];

/** The local worktrees of `wt`'s repository, `wt` first (F-WT-09 side by side). */
function previewSiblings(wt: WorkspaceSummary): string[] {
  const others = useWorkspaces
    .getState()
    .list.filter(
      (w) => w.id !== wt.id && w.parentId === wt.parentId && w.laneBranch && w.kind === "local" && !w.missing,
    )
    .map((w) => w.id);
  return [wt.id, ...others];
}

/** The worktree menu (rail row ⋯ and the pane ⋯ menu, UI_SPEC §4.3). */
export function worktreeMenu(wt: WorkspaceSummary, base: string, show: Show): MenuEntry[] {
  const open = useWorktrees.getState().open;
  return [
    {
      id: "wt-diff",
      label: t("worktree.openDiff"),
      onSelect: () => open({ kind: "diff", worktreeId: wt.id }),
    },
    {
      // F-CD-15 "Open in Files": the worktree's folder in the Files tab.
      id: "wt-files",
      label: t("worktree.openInFiles"),
      disabled: wt.missing,
      onSelect: () => {
        switchMode("code");
        useWorkspaces.getState().select(wt.id);
        useSidebar.getState().openTab("files");
      },
    },
    {
      id: "wt-merge",
      label: t("worktree.merge", { base }),
      disabled: wt.missing,
      onSelect: () => open({ kind: "merge", worktreeId: wt.id }),
    },
    {
      id: "wt-pr",
      label:
        useWorktrees.getState().forges[wt.id] === "gitlab" ? t("worktree.createMr") : t("worktree.createPr"),
      disabled: wt.missing,
      onSelect: () =>
        void createPullRequest(wt).then((out) => {
          if (out.ok) switchMode("code");
          else show({ tone: "error", title: out.error.message });
        }),
    },
    {
      // F-WT-09: the repository's preview, in a Preview pane in the worktree.
      id: "wt-preview",
      label: t("worktree.preview.start"),
      disabled: wt.missing || wt.kind !== "local",
      onSelect: () =>
        void startPreview(wt.id).then((out) => {
          if (out.ok) switchMode("code");
        }),
    },
    {
      // F-WT-09: this worktree's preview beside another one's.
      id: "wt-compare-previews",
      label: t("worktree.compare.menu"),
      disabled: wt.missing || wt.kind !== "local" || previewSiblings(wt).length < 2,
      onSelect: () => open({ kind: "comparePreviews", candidates: previewSiblings(wt), chosen: [wt.id] }),
    },
    {
      // F-WT-03: sibling worktrees on the same files, and the merge forecast.
      id: "wt-overlap",
      label: t("worktree.overlap.menu"),
      // On an SSH host these need its git and files locally (D-2209).
      disabled: wt.missing || wt.kind !== "local",
      onSelect: () => open({ kind: "overlap", worktreeId: wt.id }),
    },
    {
      // F-WT-05: another engine reads the diff and leaves line comments.
      id: "wt-second-opinion",
      label: t("worktree.opinion.menu"),
      disabled: wt.missing,
      onSelect: () => open({ kind: "opinion", worktreeId: wt.id }),
    },
    {
      // F-WT-01: the repository's setup, from any of its worktrees.
      id: "wt-setup",
      label: t("worktree.setup.menu"),
      // On an SSH host these need its git and files locally (D-2209).
      disabled: wt.missing || wt.kind !== "local",
      onSelect: () => open({ kind: "setup", workspaceId: wt.id }),
    },
    { id: "wt-sep", type: "separator" },
    {
      id: "wt-discard",
      label: t("worktree.discard"),
      danger: true,
      onSelect: () => open({ kind: "discard", worktreeId: wt.id }),
    },
  ];
}

/** A worktree under its repository in the Code rail (F-CD-15 AC4). */
export function WorktreeRow({
  wt,
  base,
  slot,
}: {
  wt: WorkspaceSummary;
  base: string;
  slot?: React.ReactNode;
}) {
  const activeId = useWorkspaces((s) => s.activeId);
  const { show } = useToast();
  const branch = wt.laneBranch ?? wt.name;
  // F-WT-04: the menu says pull or merge request by `origin`'s host.
  useWorktrees((s) => s.forges[wt.id]);
  useEffect(() => {
    if (!wt.missing) void loadForge(wt.id);
    // F-WT-06: the row's tooltip names its port block.
    if (!wt.missing && wt.kind === "local") void loadRuntime(wt.id);
  }, [wt.id, wt.missing, wt.kind]);
  const block = useWorktrees((s) => s.runtimes[wt.id]);
  const preview = useWorktrees((s) => s.previews[wt.id]);
  const openIt = () => {
    useWorkspaces.getState().select(wt.id);
    switchMode("code");
  };
  return (
    <div
      role="treeitem"
      aria-level={2}
      aria-selected={activeId === wt.id}
      tabIndex={-1}
      data-testid="worktree-row"
      data-branch={branch}
      onClick={openIt}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          openIt();
        }
      }}
      className={cx(
        "group/wt flex h-7 w-full cursor-default items-center gap-1.5 rounded-sm px-2 text-left text-sm outline-none hover:bg-surface-4 focus-visible:shadow-focus",
        activeId === wt.id ? "bg-surface-4 text-fg" : "text-fg-muted hover:text-fg",
      )}
    >
      {wt.missing ? (
        <AlertTriangle size={12} aria-label={t("rail.missing")} className="shrink-0 text-warning" />
      ) : (
        <GitBranch size={12} aria-hidden="true" className="shrink-0" />
      )}
      <Tooltip
        label={
          wt.missing
            ? t("rail.missing")
            : block
              ? t("worktree.runtime.tooltip", {
                  root: wt.root,
                  start: String(block.start),
                  end: String(block.end),
                })
              : wt.root
        }
      >
        <span className="min-w-0 flex-1 truncate">{branch}</span>
      </Tooltip>
      {slot}
      {preview?.status === "ready" && preview.url ? (
        <span onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
          <Button
            size="sm"
            variant="ghost"
            data-testid="worktree-preview"
            aria-label={t("worktree.preview.openLabel", { branch })}
            onClick={() => openPreview(preview.url ?? "")}
          >
            {t("worktree.preview.open")}
          </Button>
        </span>
      ) : null}
      {wt.liveCount > 0 && (
        <span aria-label={t("rail.live", { n: wt.liveCount })}>
          <Badge>{wt.liveCount}</Badge>
        </span>
      )}
      <span
        className="opacity-0 group-hover/wt:opacity-100 group-focus-within/wt:opacity-100"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
      >
        <Menu
          align="end"
          trigger={
            <IconButton
              size="sm"
              icon={<MoreHorizontal size={14} />}
              label={t("worktree.menuLabel", { branch })}
            />
          }
          entries={worktreeMenu(wt, base, show)}
        />
      </span>
    </div>
  );
}

/** The pane header chip "⎇ tandem/{slug}"; hovering shows the path. */
export function WorktreeChip({ wt }: { wt: WorkspaceSummary }) {
  return (
    <Tooltip label={wt.root}>
      <span
        data-testid="worktree-chip"
        tabIndex={0}
        className="inline-flex max-w-[160px] shrink-0 items-center gap-0.5 rounded-xs text-xs text-fg-subtle focus-visible:shadow-focus focus-visible:outline-none"
      >
        <GitBranch size={11} aria-hidden="true" className="shrink-0" />
        <span className="truncate">{wt.laneBranch}</span>
      </span>
    </Tooltip>
  );
}

/** "New worktree…": a name for the branch, from the repository's HEAD. */
function NewWorktreeDialog({ ws, onClose }: { ws: WorkspaceSummary; onClose: () => void }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { show } = useToast();
  const submit = async () => {
    setBusy(true);
    setError(null);
    const out = await createWorktree(ws.id, text);
    setBusy(false);
    if (out.ok) {
      show({ tone: "success", title: t("worktree.created", { branch: out.value.laneBranch ?? "" }) });
      onClose();
      // F-WT-01: report the copy, run a trusted setup or ask for a review.
      void prepareWorktree(out.value.id);
    } else setError(out.error.message);
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("worktree.newTitle")}
      description={t("worktree.newDescription", { folder: ws.name, base: ws.branch ?? "HEAD" })}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            loading={busy}
            onClick={() => void submit()}
            data-testid="worktree-create"
          >
            {t("worktree.create")}
          </Button>
        </>
      }
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <Input
          label={t("worktree.newField")}
          placeholder={t("worktree.newPlaceholder")}
          value={text}
          autoFocus
          maxLength={200}
          onChange={(e) => setText(e.target.value)}
          error={error ?? undefined}
          data-testid="worktree-name"
        />
      </form>
    </Dialog>
  );
}

function DiffDialog({ wt, onClose }: { wt: WorkspaceSummary; onClose: () => void }) {
  const [files, setFiles] = useState<FileDiff[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    void worktreeDiff(wt.id).then((out) => {
      if (!live) return;
      if (out.ok) setFiles(out.value);
      else setError(out.error.message);
    });
    return () => {
      live = false;
    };
  }, [wt.id]);
  const adds = files?.reduce((n, f) => n + f.additions, 0) ?? 0;
  const dels = files?.reduce((n, f) => n + f.deletions, 0) ?? 0;
  return (
    <Dialog
      open
      size="lg"
      onOpenChange={(o) => !o && onClose()}
      title={t("worktree.diffTitle", { branch: wt.laneBranch ?? "" })}
      description={
        files ? t("worktree.diffSummary", { n: files.length, adds, dels }) : t("worktree.diffDescription")
      }
      closeLabel={t("common.close")}
    >
      <div data-testid="worktree-diff" className="max-h-[60vh] overflow-y-auto">
        {error ? (
          <p className="text-sm text-error">{error}</p>
        ) : files === null ? (
          <div className="flex flex-col gap-2" aria-busy="true">
            <Skeleton height={32} />
            <Skeleton height={96} />
          </div>
        ) : (
          <ReviewDiff
            wt={wt}
            files={files}
            onSecondOpinion={() => useWorktrees.getState().open({ kind: "opinion", worktreeId: wt.id })}
            labels={{
              empty: t("worktree.diffEmpty"),
              binary: t("diff.binary"),
              truncated: t("diff.truncated"),
              showLarge: (n) => t("diff.showLarge", { n }),
              collapse: (path) => t("diff.collapse", { path }),
              expand: (path) => t("diff.expand", { path }),
            }}
          />
        )}
      </div>
    </Dialog>
  );
}

function MergeDialogs({
  wt,
  parent,
  base,
  onClose,
}: {
  wt: WorkspaceSummary;
  parent: WorkspaceSummary | undefined;
  base: string;
  onClose: () => void;
}) {
  const [report, setReport] = useState<MergeReport | null>(null);
  // The confirm closes itself on Merge; stay mounted until the report is in,
  // so a conflict can still be shown.
  const merging = useRef(false);
  const { show } = useToast();
  const branch = wt.laneBranch ?? "";
  const folder = parent?.name ?? "";
  if (report && !report.merged)
    return (
      <Dialog
        open
        onOpenChange={(o) => !o && onClose()}
        title={t("worktree.conflictTitle", { n: report.conflicts.length })}
        description={t("worktree.conflictBody", { folder })}
        closeLabel={t("common.close")}
        footer={
          <Button variant="primary" onClick={onClose}>
            {t("common.close")}
          </Button>
        }
      >
        <ul data-testid="merge-conflicts" className="flex flex-col gap-1 font-mono text-xs text-fg">
          {report.conflicts.map((path) => (
            <li key={path} className="truncate">
              <span className="mr-2 font-semibold text-error">{t("worktree.conflictMark")}</span>
              {path}
            </li>
          ))}
        </ul>
      </Dialog>
    );
  return (
    <ConfirmDialog
      open
      onOpenChange={(o) => !o && !merging.current && onClose()}
      title={t("worktree.mergeTitle", { branch, base })}
      body={t("worktree.mergeBody", { base, folder })}
      confirmLabel={t("worktree.mergeConfirm")}
      // F-WT-03: what the merge will do, before it's confirmed.
      onConfirm={() => {
        merging.current = true;
        void mergeWorktree(wt.id).then((out) => {
          merging.current = false;
          if (!out.ok) {
            show({ tone: "error", title: out.error.message });
            onClose();
          } else if (out.value.merged) {
            show({
              tone: "success",
              title: out.value.commit
                ? t("worktree.merged", { branch, base })
                : t("worktree.upToDate", { branch, base }),
            });
            void useWorkspaces.getState().refresh();
            onClose();
          } else setReport(out.value);
        });
      }}
    >
      {wt.kind === "local" ? <MergeForecast laneId={wt.id} /> : null}
    </ConfirmDialog>
  );
}

function DiscardDialog({ wt, onClose }: { wt: WorkspaceSummary; onClose: () => void }) {
  const { show } = useToast();
  const branch = wt.laneBranch ?? "";
  return (
    <ConfirmDialog
      open
      danger
      onOpenChange={(o) => !o && onClose()}
      title={t("worktree.discardTitle", { branch })}
      body={t("worktree.discardBody")}
      confirmLabel={t("worktree.discardConfirm")}
      requireText={branch}
      requireLabel={(text) => t("worktree.discardType", { text })}
      onConfirm={() => {
        void (async () => {
          // A clean worktree goes at once; uncommitted work needs the
          // force, which the core confirms natively once more.
          let out = await removeWorktree(wt.id, false);
          if (!out.ok && out.error.code === "conflict") out = await removeWorktree(wt.id, true);
          if (out.ok)
            show({
              tone: out.value?.teardown && out.value.teardown.kind !== "ran" ? "warning" : undefined,
              title: t("worktree.discarded", { branch }),
              body: teardownText(out.value?.teardown),
            });
          else if (out.error.code !== "cancelled") show({ tone: "error", title: out.error.message });
          onClose();
        })();
      }}
    />
  );
}

/** The one host for worktree dialogs, opened from rail rows, pane menus
 * and workspace menus. */
export function WorktreeDialogs() {
  const ask = useWorktrees((s) => s.ask);
  const list = useWorkspaces((s) => s.list);
  const close = () => useWorktrees.getState().open(null);
  useWorktreeNotices();
  if (!ask) return null;
  if (ask.kind === "review")
    return (
      <ReviewSetupDialog worktreeIds={ask.worktreeIds} state={ask.state} after={ask.after} onClose={close} />
    );
  if (ask.kind === "setup") {
    const ws = list.find((w) => w.id === ask.workspaceId);
    return ws ? <WorktreeSetupDialog ws={ws} onClose={close} /> : null;
  }
  if (ask.kind === "comparePreviews")
    return (
      <ComparePreviewsDialog
        candidates={ask.candidates}
        chosen={ask.chosen}
        onDone={ask.onDone}
        onClose={close}
      />
    );
  if (ask.kind === "instructions") {
    const ws = list.find((w) => w.id === ask.workspaceId);
    return ws ? <InstructionsDialog ws={ws} onClose={close} /> : null;
  }
  if (ask.kind === "new") {
    const ws = list.find((w) => w.id === ask.workspaceId);
    return ws ? <NewWorktreeDialog ws={ws} onClose={close} /> : null;
  }
  const wt = list.find((w) => w.id === ask.worktreeId);
  if (!wt) return null;
  if (ask.kind === "diff") return <DiffDialog wt={wt} onClose={close} />;
  if (ask.kind === "opinion") return <SecondOpinionDialog wt={wt} onClose={close} />;
  if (ask.kind === "overlap") return <OverlapDialog wt={wt} base={baseBranch(list, wt)} onClose={close} />;
  if (ask.kind === "merge")
    return (
      <MergeDialogs
        wt={wt}
        parent={list.find((w) => w.id === wt.parentId)}
        base={baseBranch(list, wt)}
        onClose={close}
      />
    );
  return <DiscardDialog wt={wt} onClose={close} />;
}

/** F-WT-09: asks every 2 s whether the given worktrees' previews answer,
 * while `active` (the rail is open and the window visible). */
export function usePreviewPolling(laneIds: string[], active: boolean): void {
  const key = laneIds.join(",");
  useEffect(() => {
    if (!active || !key) return;
    const ids = key.split(",");
    const tick = () => {
      if (document.visibilityState !== "hidden") void pollPreviews(ids);
    };
    tick();
    const timer = setInterval(tick, PREVIEW_POLL_MS);
    return () => clearInterval(timer);
  }, [key, active]);
}
