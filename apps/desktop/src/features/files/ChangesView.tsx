// The Files tab's Changes view (F-CD-20; states as UI-24): staged and
// unstaged changes, a diff for the picked one, stage, unstage, discard
// (asked first) and commit, plus the branch chip with Pull, Push or Publish,
// Switch branch and New branch. It covers the whole repository (D-205).
import { useState } from "react";
import { ChevronLeft, FileText, GitBranch, GitCompareArrows, Minus, Plus, Undo2 } from "lucide-react";
import type { FileChange, FileContext, FileStatus, GitPanel } from "@tandem/protocol";
import {
  Banner,
  Button,
  ConfirmDialog,
  Dialog,
  DiffView,
  EmptyState,
  GIT_LETTER,
  IconButton,
  Input,
  Menu,
  Skeleton,
  Spinner,
  Textarea,
  cx,
  shortcutLabel,
  useToast,
  type DiffViewLabels,
  type MenuEntry,
} from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { useGit, type GitView, type Picked } from "../../stores/git";

export function diffLabels(): Partial<DiffViewLabels> {
  return {
    binary: t("diff.binary"),
    truncated: t("diff.truncated"),
    empty: t("git.diff.empty"),
    showLarge: (n) => t("diff.showLarge", { n }),
    collapse: (path) => t("diff.collapse", { path }),
    expand: (path) => t("diff.expand", { path }),
  };
}

const CHANGE: Record<FileChange, MessageKey> = {
  added: "files.status.added",
  modified: "files.status.modified",
  deleted: "files.status.deleted",
  renamed: "files.status.renamed",
  type_changed: "files.status.typeChanged",
  untracked: "files.status.untracked",
  conflicted: "files.status.conflicted",
};

function baseName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

function dirName(path: string): string {
  const at = path.lastIndexOf("/");
  return at < 0 ? "" : path.slice(0, at);
}

/** Changes in total, for the view switch's label. */
export function changeCount(panel: GitPanel | null | undefined): number {
  return panel ? panel.status.staged.length + panel.status.unstaged.length : 0;
}

type Discarding = { paths: string[]; name: string | null } | null;

function Row({
  file,
  staged,
  picked,
  busy,
  onPick,
  onOpen,
  onStage,
  onUnstage,
  onDiscard,
}: {
  file: FileStatus;
  staged: boolean;
  picked: boolean;
  busy: boolean;
  onPick: () => void;
  onOpen: () => void;
  onStage: () => void;
  onUnstage: () => void;
  onDiscard: () => void;
}) {
  const name = baseName(file.path);
  const letter = GIT_LETTER[file.change];
  return (
    <li
      className={cx(
        "group flex h-7 items-center gap-1 rounded-sm pl-2 pr-1",
        picked ? "bg-surface-3" : "hover:bg-surface-2",
      )}
      data-testid="git-change"
      data-path={file.path}
      data-staged={staged || undefined}
    >
      <button
        type="button"
        onClick={onPick}
        aria-pressed={picked}
        aria-label={t("git.row", { name, change: t(CHANGE[file.change]) })}
        title={file.path}
        className="flex min-w-0 flex-1 items-center gap-2 rounded-xs text-left text-sm outline-none focus-visible:shadow-focus"
      >
        <span
          aria-hidden="true"
          className={cx("w-3 shrink-0 text-center font-mono text-xs font-semibold", letter.tone)}
        >
          {letter.letter}
        </span>
        <span className="truncate text-fg">{name}</span>
        {dirName(file.path) && <span className="truncate text-xs text-fg-subtle">{dirName(file.path)}</span>}
      </button>
      <span className="flex shrink-0 items-center opacity-0 group-focus-within:opacity-100 group-hover:opacity-100">
        {file.change !== "deleted" && (
          <IconButton
            size="sm"
            icon={<FileText size={13} />}
            label={t("git.open", { name })}
            onClick={onOpen}
          />
        )}
        {!staged && file.change !== "conflicted" && (
          <IconButton
            size="sm"
            icon={<Undo2 size={13} />}
            label={t("git.discard", { name })}
            disabled={busy}
            data-testid="git-discard"
            onClick={onDiscard}
          />
        )}
        {staged ? (
          <IconButton
            size="sm"
            icon={<Minus size={13} />}
            label={t("git.unstage", { name })}
            disabled={busy}
            data-testid="git-unstage"
            onClick={onUnstage}
          />
        ) : (
          <IconButton
            size="sm"
            icon={<Plus size={13} />}
            label={t("git.stage", { name })}
            disabled={busy}
            data-testid="git-stage"
            onClick={onStage}
          />
        )}
      </span>
    </li>
  );
}

/** The list, the commit box and the section actions. */
export function ChangesList({
  context,
  view,
  onOpenFile,
}: {
  context: FileContext;
  view: GitView | undefined;
  /** Opens a repository-relative path in the editor. */
  onOpenFile: (path: string) => void;
}) {
  const { show } = useToast();
  const [discarding, setDiscarding] = useState<Discarding>(null);
  const git = useGit.getState;
  const report = (err: { message: string } | null) => {
    if (err) show({ tone: "error", title: err.message });
  };

  if (!view || view.at === "loading") {
    return (
      <div className="flex flex-col gap-2 p-3" aria-busy="true" data-testid="git-loading">
        {[0, 1, 2].map((i) => (
          <Skeleton key={i} className="h-6" />
        ))}
      </div>
    );
  }
  if (view.at === "error") {
    return (
      <div className="p-3">
        <Banner tone="error" action={{ label: t("git.retry"), onClick: () => void git().refresh(context) }}>
          {t("git.failed", { reason: view.error ?? "" })}
        </Banner>
      </div>
    );
  }
  const panel = view.panel;
  if (!panel) {
    return (
      <EmptyState
        icon={<GitBranch size={18} />}
        title={t("git.notRepo.title")}
        body={t("git.notRepo.body")}
      />
    );
  }

  const { staged, unstaged } = panel.status;
  const busy = view.busy !== null;
  const canCommit = staged.length > 0 && view.message.trim().length > 0 && !busy;
  const commit = async () => {
    if (!canCommit) return;
    report(await git().commit(context));
  };
  const pick = (picked: Picked) => void git().pick(context, picked);
  const isPicked = (path: string, isStaged: boolean) =>
    view.picked?.path === path && view.picked.staged === isStaged;

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="git-changes">
      <div className="flex flex-col gap-2 border-b border-divider p-2">
        <Textarea
          label={t("git.commit.label")}
          hideLabel
          placeholder={t("git.commit.placeholder", { keys: shortcutLabel(["Mod", "Enter"]) })}
          minRows={2}
          maxRows={8}
          value={view.message}
          data-testid="git-message"
          onChange={(e) => git().setMessage(context, e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              void commit();
            }
          }}
        />
        <Button
          size="sm"
          variant="primary"
          disabled={!canCommit}
          loading={view.busy === "commit"}
          data-testid="git-commit"
          onClick={() => void commit()}
        >
          {staged.length > 0 ? t("git.commit.button", { n: staged.length }) : t("git.commit.buttonNone")}
        </Button>
        {staged.length === 0 && unstaged.length > 0 && (
          <p className="text-xs text-fg-subtle">{t("git.commit.stageFirst")}</p>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-1">
        {staged.length === 0 && unstaged.length === 0 && (
          <EmptyState title={t("git.clean.title")} body={t("git.clean.body")} />
        )}
        {staged.length > 0 && (
          <section aria-label={t("git.staged.title", { n: staged.length })} className="mb-2">
            <div className="flex h-7 items-center gap-1 px-2">
              <h3 className="flex-1 text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
                {t("git.staged.title", { n: staged.length })}
              </h3>
              <IconButton
                size="sm"
                icon={<Minus size={13} />}
                label={t("git.unstageAll")}
                disabled={busy}
                data-testid="git-unstage-all"
                onClick={() =>
                  void git()
                    .unstage(
                      context,
                      staged.map((f) => f.path),
                    )
                    .then(report)
                }
              />
            </div>
            <ul className="flex flex-col">
              {staged.map((f) => (
                <Row
                  key={`s:${f.path}`}
                  file={f}
                  staged
                  picked={isPicked(f.path, true)}
                  busy={busy}
                  onPick={() => pick({ path: f.path, staged: true })}
                  onOpen={() => onOpenFile(f.path)}
                  onStage={() => undefined}
                  onUnstage={() => void git().unstage(context, [f.path]).then(report)}
                  onDiscard={() => undefined}
                />
              ))}
            </ul>
          </section>
        )}
        {unstaged.length > 0 && (
          <section aria-label={t("git.unstaged.title", { n: unstaged.length })}>
            <div className="flex h-7 items-center gap-1 px-2">
              <h3 className="flex-1 text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
                {t("git.unstaged.title", { n: unstaged.length })}
              </h3>
              <IconButton
                size="sm"
                icon={<Undo2 size={13} />}
                label={t("git.discardAll")}
                disabled={busy}
                data-testid="git-discard-all"
                onClick={() =>
                  setDiscarding({
                    paths: unstaged.filter((f) => f.change !== "conflicted").map((f) => f.path),
                    name: null,
                  })
                }
              />
              <IconButton
                size="sm"
                icon={<Plus size={13} />}
                label={t("git.stageAll")}
                disabled={busy}
                data-testid="git-stage-all"
                onClick={() =>
                  void git()
                    .stage(
                      context,
                      unstaged.map((f) => f.path),
                    )
                    .then(report)
                }
              />
            </div>
            <ul className="flex flex-col">
              {unstaged.map((f) => (
                <Row
                  key={`u:${f.path}`}
                  file={f}
                  staged={false}
                  picked={isPicked(f.path, false)}
                  busy={busy}
                  onPick={() => pick({ path: f.path, staged: false })}
                  onOpen={() => onOpenFile(f.path)}
                  onStage={() => void git().stage(context, [f.path]).then(report)}
                  onUnstage={() => undefined}
                  onDiscard={() => setDiscarding({ paths: [f.path], name: baseName(f.path) })}
                />
              ))}
            </ul>
          </section>
        )}
      </div>
      <ConfirmDialog
        open={discarding !== null}
        onOpenChange={(o) => !o && setDiscarding(null)}
        danger
        title={
          discarding?.name
            ? t("git.discardOne.title", { name: discarding.name })
            : t("git.discardAll.title", { n: discarding?.paths.length ?? 0 })
        }
        body={t("git.discard.body")}
        confirmLabel={t("git.discard.confirm")}
        onConfirm={() => {
          const paths = discarding?.paths ?? [];
          setDiscarding(null);
          if (paths.length) void git().discard(context, paths).then(report);
        }}
      />
    </div>
  );
}

/** The picked change's diff, or what to do to get one. */
export function ChangeDiff({ view, onBack }: { view: GitView | undefined; onBack?: () => void }) {
  const diff = view?.diff ?? null;
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-testid="git-diff-pane">
      {onBack && (
        <div className="flex h-8 shrink-0 items-center gap-1 border-b border-divider px-1">
          <IconButton
            size="sm"
            icon={<ChevronLeft size={14} />}
            label={t("git.diff.back")}
            onClick={onBack}
          />
          {diff && <span className="truncate text-sm text-fg">{baseName(diff.picked.path)}</span>}
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto p-2">
        {!diff ? (
          <EmptyState icon={<GitCompareArrows size={18} />} title={t("git.diff.pick")} />
        ) : diff.at === "loading" ? (
          <div className="flex items-center gap-2 p-2 text-sm text-fg-muted">
            <Spinner size={12} /> {t("git.diff.loading")}
          </div>
        ) : diff.at === "error" ? (
          <Banner tone="error">{t("git.diff.failed", { reason: diff.error ?? "" })}</Banner>
        ) : diff.diff ? (
          <div
            data-testid="git-diff"
            data-path={diff.picked.path}
            data-staged={diff.picked.staged || undefined}
          >
            <p className="mb-2 text-xs text-fg-subtle">
              {diff.picked.staged ? t("git.diff.staged") : t("git.diff.unstaged")}
            </p>
            <DiffView files={[diff.diff]} labels={diffLabels()} />
          </div>
        ) : null}
      </div>
    </div>
  );
}

function NewBranchDialog({
  context,
  from,
  onClose,
}: {
  context: FileContext;
  from: string | null;
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { show } = useToast();
  const submit = async () => {
    const clean = name.trim();
    if (!clean) {
      setError(t("git.newBranch.empty"));
      return;
    }
    setBusy(true);
    const err = await useGit.getState().createBranch(context, clean);
    setBusy(false);
    if (err) setError(err.message);
    else {
      show({ tone: "success", title: t("git.switched", { branch: clean }) });
      onClose();
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("git.newBranch.title")}
      description={from ? t("git.newBranch.from", { branch: from }) : undefined}
      size="sm"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("git.cancel")}
          </Button>
          <Button
            variant="primary"
            loading={busy}
            data-testid="git-new-branch-create"
            onClick={() => void submit()}
          >
            {t("git.newBranch.create")}
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
          label={t("git.newBranch.label")}
          value={name}
          autoFocus
          spellCheck={false}
          error={error ?? undefined}
          data-testid="git-new-branch-name"
          onChange={(e) => {
            setName(e.target.value);
            setError(null);
          }}
        />
      </form>
    </Dialog>
  );
}

/** The branch with ahead/behind, and its menu. */
export function BranchChip({ context, view }: { context: FileContext; view: GitView | undefined }) {
  const { show } = useToast();
  const [naming, setNaming] = useState(false);
  const panel = view?.panel;
  if (!panel) return null;
  const s = panel.status;
  const git = useGit.getState;
  const name = s.branch ?? t("git.branch.detached");
  const busy = view?.busy ?? null;
  const branches = view?.branches ?? null;
  const done = (err: { message: string } | null, success: string) =>
    show(err ? { tone: "error", title: err.message } : { tone: "success", title: success });

  const others = (branches ?? []).filter((b) => !b.current);
  // A remote folder: the host's own sign-in pushes and pulls (D-411).
  const entries: MenuEntry[] = panel.host
    ? [{ id: "remote", label: t("git.remoteOnly", { host: panel.host }), disabled: true }]
    : [
        {
          id: "pull",
          label: t("git.pull"),
          disabled: !s.upstream || busy !== null,
          onSelect: () =>
            void git()
              .pull(context)
              .then((err) => done(err, t("git.pulled", { branch: name }))),
        },
        s.upstream
          ? {
              id: "push",
              label: t("git.push"),
              disabled: s.noCommits || busy !== null,
              onSelect: () =>
                void git()
                  .push(context)
                  .then((err) => done(err, t("git.pushed", { branch: name }))),
            }
          : {
              id: "publish",
              label: t("git.publish"),
              disabled: s.noCommits || !s.branch || busy !== null,
              onSelect: () =>
                void git()
                  .push(context, true)
                  .then((err) => done(err, t("git.published", { branch: name }))),
            },
        { type: "separator", id: "sep" },
        {
          id: "switch",
          label: t("git.switch"),
          disabled: busy !== null,
          submenu:
            branches === null
              ? [{ id: "loading", label: t("git.switch.loading"), disabled: true }]
              : others.length === 0
                ? [{ id: "none", label: t("git.switch.none"), disabled: true }]
                : others.map((b) => ({
                    id: `b:${b.name}`,
                    label: b.name,
                    onSelect: () =>
                      void git()
                        .switchTo(context, b.name)
                        .then((err) => done(err, t("git.switched", { branch: b.name }))),
                  })),
        },
        { id: "new", label: t("git.newBranch"), disabled: busy !== null, onSelect: () => setNaming(true) },
      ];

  const counts = s.upstream
    ? t("git.branch.counts", { ahead: s.ahead, behind: s.behind })
    : t("git.branch.unpublished");
  return (
    <>
      <Menu
        align="end"
        onOpenChange={(open) => open && !panel.host && void git().loadBranches(context)}
        trigger={
          <button
            type="button"
            data-testid="git-branch-chip"
            data-branch={s.branch ?? ""}
            data-ahead={s.ahead}
            data-behind={s.behind}
            data-upstream={s.upstream ?? ""}
            aria-label={t("git.branch.label", { name, counts })}
            className="inline-flex h-6 max-w-[220px] items-center gap-1.5 rounded-full border border-border px-2 text-xs text-fg-muted hover:border-border-strong hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
          >
            {busy === "push" || busy === "pull" || busy === "switch" ? (
              <Spinner size={12} />
            ) : (
              <GitBranch size={12} aria-hidden="true" />
            )}
            <span className="truncate font-medium text-fg">{name}</span>
            <span aria-hidden="true" className="shrink-0 tabular-nums">
              {s.upstream
                ? t("git.branch.short", { ahead: s.ahead, behind: s.behind })
                : t("git.branch.unpublished")}
            </span>
          </button>
        }
        entries={entries}
      />
      {naming && <NewBranchDialog context={context} from={s.branch} onClose={() => setNaming(false)} />}
    </>
  );
}
