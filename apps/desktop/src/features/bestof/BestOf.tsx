// Best of N (F-EX-04): the sheet that starts it (UI-143) and the compare view
// (UI-144): one column per agent with its state, its diff against the
// starting commit and its check, then "Keep this one…" or "Discard all…".
import { useCallback, useEffect, useMemo, useState } from "react";
import { create } from "zustand";
import type { BestOfDetail, BestOfRun, BestOfSeat, GitPanel, IpcError, MergeReport } from "@tandem/protocol";
import {
  Badge,
  Banner,
  Button,
  Callout,
  Dialog,
  DiffView,
  EmptyState,
  Input,
  Select,
  Sheet,
  Skeleton,
  Spinner,
  Textarea,
  cx,
  useToast,
  type SelectOption,
} from "@tandem/ui";
import { t } from "../../i18n";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import { useLayout } from "../../stores/layout";
import { paneKey, useStatus } from "../../stores/status";
import { useWorkspaces } from "../../stores/workspaces";
import { openPreview, startPreview, useWorktrees } from "../../stores/worktrees";
import { usePreviewPolling } from "../code/Worktrees";
import { diffLabels } from "../files/ChangesView";

type BestOfStore = {
  sheet: string | null;
  compare: string | null;
  openSheet: (workspaceId: string) => void;
  openCompare: (runId: string) => void;
  close: () => void;
};

export const useBestOf = create<BestOfStore>((set) => ({
  sheet: null,
  compare: null,
  openSheet: (workspaceId) => set({ sheet: workspaceId, compare: null }),
  openCompare: (runId) => set({ compare: runId, sheet: null }),
  close: () => set({ sheet: null, compare: null }),
}));

/** The newest running run in a workspace (the palette's "Compare Best of N"). */
export async function latestRun(workspaceId: string | null): Promise<BestOfRun | null> {
  const runs = await invoke<BestOfRun[]>("bestof_list", { workspaceId }).catch(() => []);
  return runs.find((r) => r.state === "running") ?? null;
}

/** "tandem/fix-login-1 … -3" for the summary line. */
export function branchPreview(prompt: string, n: number): string {
  const slug =
    prompt
      .toLowerCase()
      .normalize("NFKD")
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 40)
      .replace(/-+$/g, "") || "session";
  return n > 1 ? `tandem/${slug}-1 … -${n}` : `tandem/${slug}`;
}

/** Shows the new lanes in the rail with their panes, focusing the first. */
async function placeLanes(run: BestOfRun, parentId: string): Promise<void> {
  const ws = useWorkspaces.getState();
  await ws.refresh();
  const lanes = run.lanes.map((l) => l.laneId).filter((x): x is string => x !== null);
  for (const id of lanes) await useLayout.getState().load(id).catch(() => undefined);
  useWorkspaces.getState().setCollapsed(parentId, false);
}

function SheetBody({ workspaceId }: { workspaceId: string }) {
  const engines = useEngines((s) => s.list);
  const installed = engines.filter((e) => e.install && !e.unsupportedReason);
  const first = installed[0]?.spec.id ?? "";
  const [prompt, setPrompt] = useState("");
  const [seats, setSeats] = useState<BestOfSeat[]>(() => [
    { engineId: first, accountId: null },
    { engineId: installed[1]?.spec.id ?? first, accountId: null },
  ]);
  const enginesLoaded = useEngines((s) => s.loaded);
  useEffect(() => {
    if (!enginesLoaded) void useEngines.getState().refresh();
  }, [enginesLoaded]);
  // Engines may finish loading after the sheet opens: empty seats take the
  // first installed one then.
  useEffect(() => {
    if (first) setSeats((all) => (all.some((x) => !x.engineId) ? all.map((x) => (x.engineId ? x : { ...x, engineId: first })) : all));
  }, [first]);
  const [check, setCheck] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // null while reading; false outside a repository.
  const [git, setGit] = useState<{ repo: boolean; dirty: boolean } | null>(null);
  useEffect(() => {
    let live = true;
    invoke<GitPanel | null>("git_status", { context: { type: "workspace", workspaceId } })
      .then((p) => live && setGit({ repo: p !== null, dirty: p !== null && p.status.staged.length + p.status.unstaged.length > 0 }))
      .catch(() => live && setGit({ repo: true, dirty: false }));
    return () => {
      live = false;
    };
  }, [workspaceId]);
  const dirty = git?.dirty === true;
  const notRepo = git?.repo === false;
  const options: SelectOption[] = engines.map((e) => ({
    value: e.spec.id,
    label: e.install ? e.spec.label : t("launch.notInstalled", { label: e.spec.label }),
    disabled: e.install === null || e.unsupportedReason !== null,
  }));
  const start = async () => {
    setBusy(true);
    setError(null);
    try {
      const run = await invoke<BestOfRun>("bestof_start", { workspaceId, prompt, seats, checkCommand: check.trim() || null }, { timeoutMs: 120_000 });
      await placeLanes(run, workspaceId);
      useBestOf.getState().openCompare(run.id);
    } catch (e) {
      setError((e as IpcError).message ?? String(e));
    } finally {
      setBusy(false);
    }
  };
  const ready = !busy && !notRepo && git !== null && prompt.trim() !== "" && seats.every((x) => x.engineId);
  if (notRepo)
    return (
      <div data-testid="bestof-sheet" data-state="not-repo">
        <EmptyState title={t("bestof.notRepo")} />
      </div>
    );
  return (
    <div
      className="flex flex-col gap-4"
      data-testid="bestof-sheet"
      onKeyDown={(e) => {
        if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && ready) {
          e.preventDefault();
          void start();
        }
      }}
    >
      <Textarea
        label={t("bestof.prompt")}
        placeholder={t("bestof.prompt.placeholder")}
        value={prompt}
        rows={5}
        onChange={(e) => setPrompt(e.target.value)}
        data-testid="bestof-prompt"
      />
      <ol className="flex flex-col gap-2" aria-label={t("bestof.seats")}>
        {seats.map((s, i) => (
          <li key={i} className="flex items-center gap-2" data-testid="bestof-seat">
            <span className="w-4 text-right text-sm tabular-nums text-fg-subtle">{i + 1}</span>
            <Select
              size="sm"
              className="min-w-0 flex-1"
              ariaLabel={t("bestof.seat", { n: i + 1 })}
              value={s.engineId}
              options={options}
              onValueChange={(v) => setSeats(seats.map((x, k) => (k === i ? { ...x, engineId: v } : x)))}
            />
            {seats.length > 2 && (
              <Button size="sm" variant="ghost" onClick={() => setSeats(seats.filter((_, k) => k !== i))}>
                {t("bestof.remove")}
              </Button>
            )}
          </li>
        ))}
      </ol>
      {seats.length < 3 && (
        <Button size="sm" variant="secondary" className="self-start" data-testid="bestof-add" onClick={() => setSeats([...seats, { engineId: first, accountId: null }])}>
          {t("bestof.add")}
        </Button>
      )}
      <Input
        label={t("bestof.check")}
        placeholder="pnpm test"
        value={check}
        spellCheck={false}
        onChange={(e) => setCheck(e.target.value)}
        data-testid="bestof-check"
      />
      {dirty && <Callout tone="info">{t("bestof.dirty")}</Callout>}
      <p className="text-sm text-fg-muted" data-testid="bestof-summary">
        {t("bestof.summary", { n: seats.length, branches: branchPreview(prompt, seats.length) })}
      </p>
      {error && <Banner tone="error">{error}</Banner>}
      <div className="flex justify-end gap-2">
        <Button variant="ghost" onClick={() => useBestOf.getState().close()}>
          {t("bestof.cancel")}
        </Button>
        <Button variant="primary" loading={busy} disabled={!ready && !busy} data-testid="bestof-start" onClick={() => void start()}>
          {busy ? t("bestof.starting", { n: seats.length }) : t("bestof.start", { n: seats.length })}
        </Button>
      </div>
    </div>
  );
}

function agentState(paneId: string | null, records: ReturnType<typeof useStatus.getState>["records"]): string {
  const s = paneId ? records[paneKey(paneId)]?.status : undefined;
  if (s === "working") return t("bestof.state.working");
  if (s === "waiting") return t("bestof.state.waiting");
  if (s === "done") return t("bestof.state.done");
  return t("bestof.state.idle");
}

function Column({
  detail,
  seat,
  file,
  busy,
  starting,
  onKeep,
  onOutput,
}: {
  detail: BestOfDetail;
  seat: number;
  file: string;
  busy: boolean;
  /** F-WT-09: Previews asked for this lane's preview. */
  starting: boolean;
  onKeep: () => void;
  onOutput: (text: string) => void;
}) {
  const lane = detail.run.lanes.find((l) => l.seat === seat);
  const diff = detail.diffs.find((d) => d.seat === seat);
  const records = useStatus((s) => s.records);
  const previews = useWorktrees((s) => s.previews);
  const label = useEngines((s) => s.list.find((e) => e.spec.id === lane?.engineId)?.spec.label ?? lane?.engineId ?? "");
  if (!lane) return null;
  const files = (diff?.files ?? []).filter((f) => !file || f.path === file);
  const add = (diff?.files ?? []).reduce((n, f) => n + f.additions, 0);
  const del = (diff?.files ?? []).reduce((n, f) => n + f.deletions, 0);
  const check = lane.check;
  const preview = lane.laneId ? previews[lane.laneId] : undefined;
  return (
    <section
      className="flex min-w-0 flex-1 flex-col rounded-md border border-border bg-surface-2"
      data-testid="bestof-column"
      data-seat={seat}
      data-branch={lane.branch ?? ""}
      data-files={diff?.files.length ?? 0}
      data-check={check}
      aria-label={label}
    >
      <header className="flex flex-col gap-1 border-b border-divider p-2">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium text-fg">{label}</span>
          {lane.branch && <span className="truncate font-mono text-xs text-fg-subtle">{lane.branch}</span>}
        </div>
        {lane.error ? (
          <Banner tone="error">{lane.error}</Banner>
        ) : (
          <div className="flex flex-wrap items-center gap-2 text-sm text-fg-muted">
            <Badge variant="outline">{agentState(lane.paneId, records)}</Badge>
            <span className="tabular-nums">{t("bestof.files", { n: diff?.files.length ?? 0, add, del })}</span>
            {check === "running" && (
              <span className="inline-flex items-center gap-1">
                <Spinner size={12} /> {t("bestof.check.running")}
              </span>
            )}
            {check === "passed" && <Badge variant="done">{t("bestof.check.passed")}</Badge>}
            {check === "failed" && <Badge variant="error">{t("bestof.check.failed", { code: lane.checkExit ?? 0 })}</Badge>}
            {check === "stopped" && <Badge variant="outline">{t("bestof.check.stopped")}</Badge>}
            {preview?.status === "ready" && preview.url ? (
              <Button
                size="sm"
                variant="secondary"
                data-testid="bestof-preview-open"
                onClick={() => openPreview(preview.url ?? "")}
              >
                {t("bestof.preview.open")}
              </Button>
            ) : preview?.status === "down" && starting ? (
              <span className="inline-flex items-center gap-1" data-testid="bestof-preview-starting">
                <Spinner size={12} /> {t("bestof.preview.starting")}
              </span>
            ) : preview?.status === "none" ? (
              <span className="text-fg-subtle" data-testid="bestof-preview-none">
                {t("bestof.preview.none")}
              </span>
            ) : null}
            {lane.checkOutput !== null && check !== "running" && (
              <Button size="sm" variant="ghost" onClick={() => onOutput(lane.checkOutput ?? "")}>
                {t("bestof.output")}
              </Button>
            )}
          </div>
        )}
      </header>
      <div className="min-h-0 flex-1 overflow-auto p-2">
        {diff?.error ? (
          <Banner tone="error">{diff.error}</Banner>
        ) : files.length ? (
          <DiffView files={files} labels={diffLabels()} />
        ) : (
          <p className="p-2 text-sm text-fg-muted">{t("bestof.noChanges")}</p>
        )}
      </div>
      {!lane.error && detail.run.state === "running" && (
        <footer className="border-t border-divider p-2">
          <Button size="sm" variant="primary" data-testid="bestof-keep" disabled={busy} onClick={onKeep}>
            {t("bestof.keep")}
          </Button>
        </footer>
      )}
    </section>
  );
}

function Compare({ runId }: { runId: string }) {
  const [detail, setDetail] = useState<BestOfDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [file, setFile] = useState("");
  const [output, setOutput] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { show } = useToast();
  const base = useWorkspaces((s) => s.list.find((w) => w.id === detail?.run.workspaceId));
  // F-WT-09: lanes whose preview tab opens once it answers.
  const [wanted, setWanted] = useState<string[]>([]);
  const previews = useWorktrees((s) => s.previews);
  const laneIds = (detail?.run.lanes ?? []).map((l) => l.laneId).filter((x): x is string => x !== null);
  usePreviewPolling(laneIds, detail?.run.state === "running");
  useEffect(() => {
    const ready = wanted.filter((id) => previews[id]?.status === "ready" && previews[id]?.url);
    if (ready.length === 0) return;
    for (const id of ready) openPreview(previews[id]?.url ?? "");
    setWanted((w) => w.filter((id) => !ready.includes(id)));
  }, [previews, wanted]);
  const startPreviews = async () => {
    const withPreview = laneIds.filter((id) => previews[id] && previews[id]?.status !== "none");
    setWanted(withPreview);
    for (const id of withPreview) if (previews[id]?.status === "down") await startPreview(id);
  };

  const load = useCallback(async () => {
    try {
      setDetail(await invoke<BestOfDetail>("bestof_get", { runId }, { timeoutMs: 60_000 }));
      setError(null);
    } catch (e) {
      setError((e as IpcError).message ?? String(e));
    }
  }, [runId]);

  useEffect(() => {
    void load();
    let off: (() => void) | undefined;
    let live = true;
    void listen<{ runId: string }>("bestof:changed", (p) => {
      if (live && p.runId === runId) void load();
    }).then((u) => {
      if (live) off = u;
      else u();
    });
    // Agents keep editing: the diffs follow every few seconds.
    const timer = setInterval(() => void load(), 5_000);
    return () => {
      live = false;
      off?.();
      clearInterval(timer);
    };
  }, [runId, load]);

  const paths = useMemo(() => {
    const all = new Set<string>();
    for (const d of detail?.diffs ?? []) for (const f of d.files) all.add(f.path);
    return [...all].sort();
  }, [detail]);

  if (error) return <Banner tone="error" action={{ label: t("bestof.retry"), onClick: () => void load() }}>{error}</Banner>;
  if (!detail)
    return (
      <div className="flex gap-2" data-testid="bestof-loading">
        <Skeleton className="h-64 flex-1" />
        <Skeleton className="h-64 flex-1" />
      </div>
    );
  const run = detail.run;
  const anyRunning = run.lanes.some((l) => l.check === "running");
  const act = async (cmd: string, args: Record<string, unknown>) => {
    setBusy(true);
    try {
      const report = await invoke<MergeReport | null>(cmd, args, { timeoutMs: 180_000 });
      if (report && !report.merged)
        show({ tone: "warning", title: t("bestof.conflicts", { n: report.conflicts.length }), body: report.conflicts.join("\n") });
      await load();
    } catch (e) {
      // Cancel in the native confirm changes nothing and says nothing.
      if ((e as IpcError).code !== "cancelled") show({ tone: "error", title: (e as IpcError).message ?? String(e) });
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex min-h-0 flex-col gap-3" data-testid="bestof-compare" data-state={run.state}>
      <div className="flex flex-wrap items-center gap-2">
        {run.state === "merged" && <Callout tone="success">{t("bestof.merged", { base: base?.name ?? "" })}</Callout>}
        {run.state === "discarded" && <Callout tone="info">{t("bestof.discarded")}</Callout>}
        {run.state === "running" && (
          <>
            <Select
              size="sm"
              className="w-64"
              ariaLabel={t("bestof.filter")}
              value={file || "__all"}
              options={[{ value: "__all", label: t("bestof.allFiles") }, ...paths.map((p) => ({ value: p, label: p }))]}
              onValueChange={(v) => setFile(v === "__all" ? "" : v)}
            />
            <span className="flex-1" />
            {run.checkCommand &&
              (anyRunning ? (
                <Button size="sm" variant="secondary" onClick={() => void act("bestof_check_stop", { runId })}>
                  {t("bestof.stopCheck")}
                </Button>
              ) : (
                <Button size="sm" variant="secondary" data-testid="bestof-run-check" onClick={() => void act("bestof_check", { runId })}>
                  {t("bestof.runCheck", { command: run.checkCommand })}
                </Button>
              ))}
            {laneIds.some((id) => previews[id] && previews[id]?.status !== "none") && (
              <Button size="sm" variant="secondary" data-testid="bestof-previews" onClick={() => void startPreviews()}>
                {t("bestof.previews")}
              </Button>
            )}
            <Button size="sm" variant="ghost" data-testid="bestof-discard" disabled={busy} onClick={() => void act("bestof_discard", { runId })}>
              {t("bestof.discardAll")}
            </Button>
          </>
        )}
      </div>
      <div className={cx("flex min-h-0 gap-2", "max-[960px]:flex-col")}>
        {run.lanes.map((l) => (
          <Column key={l.seat} detail={detail} seat={l.seat} file={file} busy={busy} starting={l.laneId !== null && wanted.includes(l.laneId)} onKeep={() => void act("bestof_keep", { runId, seat: l.seat })} onOutput={setOutput} />
        ))}
      </div>
      <Dialog open={output !== null} onOpenChange={(o) => !o && setOutput(null)} title={t("bestof.output.title")} size="lg">
        <pre className="max-h-[60vh] overflow-auto whitespace-pre-wrap font-mono text-xs text-fg" data-testid="bestof-output">
          {output}
        </pre>
      </Dialog>
    </div>
  );
}

/** The sheet and the compare view, mounted once in the shell. */
export function BestOf() {
  const { sheet, compare, close } = useBestOf();
  const ws = useWorkspaces((s) => s.list.find((w) => w.id === sheet));
  return (
    <>
      <Sheet open={sheet !== null} onOpenChange={(o) => !o && close()} title={t("bestof.title")} subtitle={ws?.name} size="md">
        {sheet && <SheetBody workspaceId={sheet} />}
      </Sheet>
      <Dialog open={compare !== null} onOpenChange={(o) => !o && close()} title={t("bestof.compare.title")} size="lg">
        {compare ? <Compare runId={compare} /> : <EmptyState title={t("bestof.none")} />}
      </Dialog>
    </>
  );
}
