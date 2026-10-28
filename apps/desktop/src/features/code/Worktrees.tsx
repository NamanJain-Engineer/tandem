import { useEffect, useRef, useState } from "react";
import { AlertTriangle, GitBranch, Globe, MoreHorizontal } from "lucide-react";
import type { FileDiff, LaneKind, LaneKinds, MergeReport, WorkspaceSummary } from "@tandem/protocol";
import {
  Badge,
  Banner,
  Button,
  ConfirmDialog,
  Dialog,
  IconButton,
  Input,
  Menu,
  SegmentedControl,
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
  allowChecks,
  baseBranch,
  createPullRequest,
  createPullRequests,
  createWorktree,
  isJj,
  laneKinds,
  trustRepository,
  PREVIEW_POLL_MS,
  loadForge,
  loadRuntime,
  loadChecks,
  openChecks,
  openPreview,
  pollPreviews,
  startPreview,
  mergeWorktree,
  removeWorktree,
  teardownText,
  useWorktrees,
  worktreeDiff,
} from "../../stores/worktrees";
import { switchMode } from "../shell/actions";
import { ComparePreviewsDialog } from "./ComparePreviews";
import { LandWorktreesDialog } from "./LandWorktrees";
import { AllowChecksDialog, ChecksBadge, ChecksDialog } from "./PullRequestChecks";
import { RadarMark } from "./IntentRadar";
import { folded, useMarkFolds } from "./markFolds";
import { PullRequestsDialog } from "./MultiPullRequests";
import { MergeForecast, OverlapDialog } from "./WorktreeOverlap";
import { SecondOpinionDialog } from "./SecondOpinion";
import { ReviewDiff } from "./WorktreeReview";
import { InstructionsDialog } from "./Instructions";
import { ReviewSetupDialog, WorktreeSetupDialog, useWorktreeNotices } from "./WorktreeSetup";
import { WorkspaceSlot } from "./WorkspaceSlot";
import { useWorkspaceStatus } from "./workspaceStatus";

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

/** Whether the worktree's repository is allowed to run gh (F-WT-08). */
function checksAllowed(id: string): boolean {
  const state = useWorktrees.getState().checks[id]?.state;
  return state !== undefined && state !== "not_allowed";
}

function parentName(wt: WorkspaceSummary): string {
  return useWorkspaces.getState().list.find((w) => w.id === wt.parentId)?.name ?? wt.name;
}

/** The worktree menu (rail row ⋯ and the pane ⋯ menu, UI_SPEC §4.3). */
/** What a multi-repo worktree's menu keeps (F-CD-22); the rest need one
 * repository and are disabled for now. */
const MULTI_KEEPS = new Set(["wt-diff", "wt-files", "wt-sep", "wt-discard"]);

/** F-CD-22: pushes and opens a linked pull request per repository. */
async function openPullRequests(wt: WorkspaceSummary, show: Show): Promise<void> {
  const out = await createPullRequests(wt.id);
  if (out.ok) useWorktrees.getState().open({ kind: "prs", worktreeId: wt.id });
  else if (out.error.code !== "cancelled") show({ tone: "error", title: out.error.message });
}

/** What a jj workspace's menu keeps (F-CD-24); the rest need git. */
const JJ_KEEPS = new Set(["wt-diff", "wt-files", "wt-overlap", "wt-sep", "wt-discard"]);

export function worktreeMenu(wt: WorkspaceSummary, base: string, show: Show): MenuEntry[] {
  const entries = singleRepoMenu(wt, base, show);
  if (isJj(wt)) {
    const kept = entries.map((e): MenuEntry => {
      if (JJ_KEEPS.has(e.id) || e.type === "separator" || e.type === "label") return e;
      return { ...e, disabled: true } as MenuEntry;
    });
    const at = kept.findIndex((e) => e.id === "wt-files") + 1;
    return [
      ...kept.slice(0, at),
      { id: "wt-jj-note", type: "label", label: t("worktree.jj.notYet") },
      ...kept.slice(at),
    ];
  }
  if ((wt.repos?.length ?? 0) === 0) return entries;
  const kept = entries.map((e): MenuEntry => {
    if (MULTI_KEEPS.has(e.id) || e.type === "separator" || e.type === "label") return e;
    return { ...e, disabled: true } as MenuEntry;
  });
  const prs: MenuEntry = {
    id: "wt-prs",
    label: t("worktree.multi.prs"),
    disabled: wt.missing || wt.kind !== "local",
    onSelect: () => void openPullRequests(wt, show),
  };
  // Create pull requests sits after Open in Files; the rest wait.
  const at = kept.findIndex((e) => e.id === "wt-files") + 1;
  return [
    ...kept.slice(0, at),
    prs,
    { id: "wt-multi-note", type: "label", label: t("worktree.multi.notYet") },
    ...kept.slice(at),
  ];
}

function singleRepoMenu(wt: WorkspaceSummary, base: string, show: Show): MenuEntry[] {
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
      // F-WT-08: the pull request's checks, after asking once per repository.
      id: "wt-checks",
      label: t("checks.menu"),
      disabled: wt.missing || wt.kind !== "local",
      onSelect: () => void openChecks(wt.id),
    },
    ...(checksAllowed(wt.id)
      ? [
          {
            id: "wt-checks-refresh",
            label: t("checks.refreshMenu"),
            onSelect: () => void loadChecks([wt.id]),
          },
          {
            id: "wt-checks-forget",
            label: t("checks.forget"),
            onSelect: () =>
              void allowChecks(wt.id, false).then((out) => {
                if (out.ok) show({ title: t("checks.forgotten", { folder: parentName(wt) }) });
              }),
          },
        ]
      : []),
    {
      // F-WT-07: this repository's worktrees, landed one after another.
      id: "wt-land",
      label: t("landing.menu"),
      disabled: wt.missing || wt.kind !== "local",
      onSelect: () => open({ kind: "land", workspaceId: wt.id }),
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

/** F-WT-09: the row's preview, while it answers; it opens in a browser tab.
 * It leads with a globe, as a browser pane's rail row does, and it's the
 * first part to fold to its icon (D-2701): its word says least. */
function PreviewMark({ url, branch, fold }: { url: string; branch: string; fold: boolean }) {
  const name = t("worktree.preview.openLabel", { branch });
  return (
    <button
      type="button"
      data-testid="worktree-preview"
      data-compact={fold ? "" : undefined}
      aria-label={name}
      title={name}
      onClick={(e) => {
        e.stopPropagation();
        openPreview(url);
      }}
      onKeyDown={(e) => e.stopPropagation()}
      className="inline-flex h-7 shrink-0 items-center gap-1 whitespace-nowrap rounded-sm px-1 text-sm font-medium text-fg hover:bg-surface-4 focus-visible:shadow-focus focus-visible:outline-none"
    >
      <Globe size={12} aria-hidden="true" className="shrink-0" />
      {!fold && <span data-words="">{t("worktree.preview.open")}</span>}
    </button>
  );
}

/** A worktree under its repository in the Code rail (F-CD-15 AC4). It's
 * named as its workspace is (a worktree's branch is `tandem/` and that
 * name), so beside a status the name keeps its room: at least 10
 * characters, as a workspace row's does (D-2603). The branch is in its
 * tooltip, and ⋯ covers the row's end only while it's hovered or focused.
 * The rail is never narrower than a fully folded row (D-2701). */
export function WorktreeRow({ wt, base }: { wt: WorkspaceSummary; base: string }) {
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
  // D-2700, D-2701: the marks fold to their icons, and the status words to
  // their dot, while the row's contents don't fit.
  const status = useWorkspaceStatus(wt);
  const checks = useWorktrees((s) => s.checks[wt.id]);
  const radar = useWorktrees((s) => s.radar[wt.id]);
  const { ref, folds } = useMarkFolds(
    JSON.stringify([
      wt.name,
      wt.missing,
      // F-CD-24's jj badge takes room in the row too.
      wt.laneVcs ?? null,
      status,
      checks?.state === "ready" ? checks.checks.map((c) => c.bucket) : null,
      radar ?? null,
      preview?.status === "ready",
      wt.liveCount,
    ]),
  );
  const openIt = () => {
    useWorkspaces.getState().select(wt.id);
    switchMode("code");
  };
  return (
    <div
      ref={ref}
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
        "group/wt relative flex h-7 w-full cursor-default items-center gap-1.5 rounded-sm px-2 text-left text-sm outline-none hover:bg-surface-4 focus-visible:shadow-focus",
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
            : `${branch} · ${
                block
                  ? t("worktree.runtime.tooltip", {
                      root: wt.root,
                      start: String(block.start),
                      end: String(block.end),
                    })
                  : wt.root
              }`
        }
      >
        <span data-testid="worktree-row-name" className="min-w-[min(10ch,100%)] flex-1 truncate">
          {wt.name}
        </span>
      </Tooltip>
      {isJj(wt) ? (
        <span data-testid="worktree-jj" aria-label={t("worktree.jj.badgeLabel")}>
          <Badge variant="outline">{t("worktree.jj.badge")}</Badge>
        </span>
      ) : null}
      <WorkspaceSlot ws={wt} fold={folded(folds, "status")} />
      {preview?.status === "ready" && preview.url ? (
        <PreviewMark url={preview.url} branch={branch} fold={folded(folds, "preview")} />
      ) : null}
      <ChecksBadge wt={wt} fold={folded(folds, "checks")} />
      <RadarMark wt={wt} fold={folded(folds, "radar")} />
      {wt.liveCount > 0 && (
        <span data-testid="worktree-live" aria-label={t("rail.live", { n: wt.liveCount })}>
          <Badge>{wt.liveCount}</Badge>
        </span>
      )}
      <span
        className="pointer-events-none absolute inset-y-0 right-1 flex items-center rounded-sm bg-surface-4 opacity-0 group-hover/wt:pointer-events-auto group-hover/wt:opacity-100 group-focus-within/wt:pointer-events-auto group-focus-within/wt:opacity-100"
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

/** The pane header chip "⎇ tandem/{slug}"; hovering shows the path. It
 * gives way before the pane's title, down to its icon (26 px; PaneChrome
 * leaves it that room), so the title keeps its 10 characters (D-2611). */
export function WorktreeChip({ wt }: { wt: WorkspaceSummary }) {
  return (
    <Tooltip label={wt.root}>
      <span
        data-testid="worktree-chip"
        tabIndex={0}
        className="inline-flex min-w-[26px] max-w-[160px] items-center gap-0.5 overflow-hidden rounded-xs text-xs text-fg-subtle focus-visible:shadow-focus focus-visible:outline-none"
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
  // F-CD-24: whether a jj workspace is offered; `undefined` while loading.
  const [kinds, setKinds] = useState<LaneKinds | null | undefined>(undefined);
  const [kind, setKind] = useState<LaneKind>("git");
  const asked = useRef<Promise<LaneKinds | null> | null>(null);
  const [asking, setAsking] = useState(0);
  const { show } = useToast();
  useEffect(() => {
    let live = true;
    // Unknown (an error, or no answer): a git worktree, as before.
    const answer = laneKinds(ws.id).then((out) => (out.ok && out.value ? out.value : null));
    asked.current = answer;
    void answer.then((k) => {
      if (!live) return;
      setKinds(k);
      if (k) setKind(k.preferred);
    });
    return () => {
      live = false;
    };
  }, [ws.id, asking]);
  // From a jj workspace, the new one is a jj workspace (F-CD-24); jj runs
  // only in a trusted repository.
  const offers = (k: LaneKinds | null | undefined) => k?.colocated === true && k.trusted && !isJj(ws);
  const trustNeeded = kinds?.colocated === true && !kinds.trusted && !isJj(ws);
  const trust = async () => {
    const out = await trustRepository(ws.id);
    if (out.ok) setAsking((n) => n + 1);
    else if (out.error.code !== "cancelled") setError(out.error.message);
  };
  const choose = offers(kinds);
  const submit = async () => {
    setBusy(true);
    setError(null);
    // Create before the answer came: wait for it, and make what it prefers.
    const known = kinds === undefined ? await asked.current : kinds;
    const chosen = kinds === undefined ? known?.preferred : kind;
    const out = await createWorktree(ws.id, text, offers(known) ? chosen : undefined);
    setBusy(false);
    if (out.ok) {
      show({ tone: "success", title: t("worktree.created", { branch: out.value.laneBranch ?? "" }) });
      onClose();
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
        className="flex flex-col gap-3"
      >
        {kinds?.jjOnly ? <Banner tone="info">{t("worktree.jj.only")}</Banner> : null}
        {kinds?.refused ? <Banner tone="warning">{kinds.refused}</Banner> : null}
        {trustNeeded ? (
          <Banner tone="info" action={{ label: t("worktree.jj.trust"), onClick: () => void trust() }}>
            {t("worktree.jj.untrusted")}
          </Banner>
        ) : null}
        {choose ? (
          <div data-testid="worktree-kind">
            <SegmentedControl
              label={t("worktree.jj.make")}
              value={kind}
              onValueChange={(id) => setKind(id === "jj" ? "jj" : "git")}
              items={[
                {
                  id: "jj",
                  label: t("worktree.jj.workspace"),
                  tooltip: t("worktree.jj.version", { version: kinds?.jjVersion ?? "" }),
                },
                { id: "git", label: t("worktree.jj.git") },
              ]}
            />
          </div>
        ) : null}
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
          <>
            {wt.repos && wt.repos.length > 0 ? (
              // F-CD-22: each repository's share of the changes; its files
              // follow under its name.
              <ul
                data-testid="diff-repos"
                className="mb-2 flex flex-wrap gap-x-4 gap-y-1 text-sm text-fg-muted"
              >
                {wt.repos.map((repo) => (
                  <li key={repo} data-testid="diff-repo" data-repo={repo}>
                    {t("worktree.multi.repoSummary", {
                      repo,
                      n: files.filter((f) => f.path.startsWith(`${repo}/`)).length,
                    })}
                  </li>
                ))}
              </ul>
            ) : null}
            <ReviewDiff
              wt={wt}
              files={files}
              onSecondOpinion={
                wt.repos && wt.repos.length > 0
                  ? undefined
                  : () => useWorktrees.getState().open({ kind: "opinion", worktreeId: wt.id })
              }
              labels={{
                empty: t("worktree.diffEmpty"),
                binary: t("diff.binary"),
                truncated: t("diff.truncated"),
                showLarge: (n) => t("diff.showLarge", { n }),
                collapse: (path) => t("diff.collapse", { path }),
                expand: (path) => t("diff.expand", { path }),
              }}
            />
          </>
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
  if (ask.kind === "allowChecks" || ask.kind === "checks") {
    const wt = list.find((w) => w.id === ask.worktreeId);
    if (!wt) return null;
    return ask.kind === "checks" ? (
      <ChecksDialog wt={wt} onClose={close} />
    ) : (
      <AllowChecksDialog wt={wt} folder={parentName(wt)} onClose={close} />
    );
  }
  if (ask.kind === "prs") {
    const wt = list.find((w) => w.id === ask.worktreeId);
    return wt ? <PullRequestsDialog wt={wt} onClose={close} /> : null;
  }
  if (ask.kind === "land") {
    const ws = list.find((w) => w.id === ask.workspaceId);
    return ws ? <LandWorktreesDialog ws={ws} laneIds={ask.laneIds} onClose={close} /> : null;
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
