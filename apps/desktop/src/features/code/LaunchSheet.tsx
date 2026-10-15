import { useEffect, useMemo, useState, type KeyboardEvent, type ReactNode } from "react";
import { Plus, X } from "lucide-react";
import type {
  Account,
  AccountOverride,
  IpcError,
  Isolation,
  LaunchReport,
  Seat,
  UsageView,
} from "@tandem/protocol";
import {
  Button,
  Callout,
  Checkbox,
  Combobox,
  Dialog,
  IconButton,
  Input,
  Menu,
  SegmentedControl,
  Select,
  Sheet,
  Skeleton,
  SkeletonGate,
  Textarea,
  shortcutLabel,
  useToast,
  type SelectOption,
} from "@tandem/ui";
import { t } from "../../i18n";
import { useEngines } from "../../stores/engines";
import { useRemote } from "../../stores/remote";
import {
  MAX_COUNT,
  ROLES,
  agentSeat,
  branchPreview,
  counts,
  defaultEngine,
  isRepo,
  launchLineup,
  lineupFrom,
  retrySeat,
  savePreset,
  shellSeat,
  browserSeat,
  useLaunch,
} from "../../stores/launch";
import { useWorkspaces } from "../../stores/workspaces";
import { accountForNext, accountsFor, useAccounts } from "../../stores/accounts";
import { useUsage } from "../../stores/usage";
import { hasRoom, isTight } from "../shell/forecast";

const CUSTOM = "custom";
const SOLO = "pst_builtin_solo";

/** "Launch 4 agents + 1 shell". */
export function launchLabel(seats: Seat[]): string {
  const c = counts(seats);
  const parts = [
    c.agents > 0 ? t("launch.agents", { n: c.agents }) : null,
    c.shells > 0 ? t("launch.shells", { n: c.shells }) : null,
    c.browsers > 0 ? t("launch.browsers", { n: c.browsers }) : null,
  ];
  return t("launch.go", { what: parts.filter(Boolean).join(" + ") });
}

function SaveDialog({
  seats,
  isolation,
  onClose,
}: {
  seats: Seat[];
  isolation: Isolation;
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const { show } = useToast();
  const save = async () => {
    const out = await savePreset(name, seats, isolation);
    if (!out.ok) setError(out.error.message);
    else {
      show({ tone: "success", title: t("launch.savedToast", { name: out.value.name }) });
      onClose();
    }
  };
  return (
    <Dialog
      open
      size="sm"
      onOpenChange={(o) => !o && onClose()}
      title={t("launch.saveTitle")}
      closeLabel={t("common.close")}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            disabled={!name.trim()}
            onClick={() => void save()}
            data-testid="preset-save"
          >
            {t("launch.saveConfirm")}
          </Button>
        </>
      }
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (name.trim()) void save();
        }}
      >
        <Input
          label={t("launch.saveName")}
          value={name}
          autoFocus
          maxLength={60}
          onChange={(e) => setName(e.target.value)}
          error={error ?? undefined}
          data-testid="preset-name"
        />
      </form>
    </Dialog>
  );
}

function Failures({
  report,
  workspaceId,
  task,
  isolation,
  onRetried,
}: {
  report: LaunchReport;
  workspaceId: string;
  task: string;
  isolation: Isolation;
  onRetried: (seatIndex: number) => void;
}) {
  const [busy, setBusy] = useState<number | null>(null);
  const [errors, setErrors] = useState<Record<number, IpcError>>({});
  return (
    <Callout tone="warning" title={t("launch.failedTitle", { n: report.failed.length })}>
      <p>{t("launch.failedBody")}</p>
      <ul data-testid="launch-failures" className="mt-2 flex flex-col gap-1.5">
        {report.failed.map((f) => (
          <li key={f.seatIndex} data-testid="launch-failure" className="flex items-center gap-2 text-sm">
            <span className="min-w-0 flex-1">
              <span className="font-medium text-fg">{t("launch.seatLabel", { n: f.seatIndex + 1 })}</span>
              {" · "}
              {(errors[f.seatIndex] ?? f.error).message}
            </span>
            <Button
              size="sm"
              variant="secondary"
              loading={busy === f.seatIndex}
              onClick={() => {
                setBusy(f.seatIndex);
                void retrySeat(workspaceId, f.seat, task, isolation, report.sharedTabId).then((out) => {
                  setBusy(null);
                  if (out.ok) onRetried(f.seatIndex);
                  else setErrors((e) => ({ ...e, [f.seatIndex]: out.error }));
                });
              }}
            >
              {t("launch.retry")}
            </Button>
          </li>
        ))}
      </ul>
    </Callout>
  );
}

/** F-AC-08: an agent seat whose account is near its limit says so, and
 * names an account of the same engine with more room. Text only: the seat
 * keeps its account (F-AC-05). */
export function seatUsageHint(
  engineId: string,
  accountId: string | null,
  workspaceId: string,
  accounts: Account[],
  overrides: AccountOverride[],
  usage: Record<string, UsageView>,
): { tight: string; room: string | null } | null {
  const mine =
    accounts.find((a) => a.id === accountId && a.engineId === engineId) ??
    accountForNext(accounts, overrides, engineId, workspaceId);
  if (!mine) return null;
  const tight = isTight(usage[mine.id]);
  if (!tight) return null;
  const other = accountsFor(accounts, engineId).find((a) => a.id !== mine.id && hasRoom(usage[a.id]));
  return {
    tight: t("launch.usageHigh", { account: mine.label, pct: tight.pct }),
    room: other ? t("launch.moreRoom", { other: other.label }) : null,
  };
}

function SeatUsageHint({
  engineId,
  accountId,
  workspaceId,
}: {
  engineId: string;
  accountId: string | null;
  workspaceId: string;
}) {
  const accounts = useAccounts((s) => s.list);
  const overrides = useAccounts((s) => s.overrides);
  const usage = useUsage((s) => s.byAccount);
  const hint = seatUsageHint(engineId, accountId, workspaceId, accounts, overrides, usage);
  if (!hint) return null;
  return (
    <span data-testid="launch-usage-hint" className="pl-6 text-xs text-warning-text">
      {hint.room ? `${hint.tight} · ${hint.room}` : hint.tight}
    </span>
  );
}

function Lineup({
  seats,
  onChange,
  engineOptions,
  workspaceId,
}: {
  seats: Seat[];
  onChange: (seats: Seat[]) => void;
  engineOptions: SelectOption[];
  workspaceId: string;
}) {
  const set = (i: number, patch: Partial<Seat>) =>
    onChange(seats.map((s, k) => (k === i ? { ...s, ...patch } : s)));
  const kindOptions: SelectOption[] = [
    ...engineOptions,
    { value: "shell", label: t("launch.shellCommand") },
    { value: "browser", label: t("launch.browserUrl") },
  ];
  const roleOptions: SelectOption[] = ROLES.map((r) => ({ value: r, label: r }));
  return (
    <ol data-testid="launch-lineup" className="flex flex-col gap-2">
      {seats.map((s, i) => {
        const n = i + 1;
        return (
          <li key={i} data-testid="launch-seat" data-kind={s.kind} className="flex flex-col gap-1">
            <div className="flex items-center gap-2">
              <span className="w-4 shrink-0 text-right text-sm tabular-nums text-fg-subtle">{n}</span>
              <Select
                size="sm"
                className="w-40 shrink-0"
                ariaLabel={t("launch.seatEngine", { n })}
                value={s.kind === "agent" ? (s.engineId ?? "") : s.kind}
                options={kindOptions}
                onValueChange={(v) =>
                  set(
                    i,
                    v === "shell"
                      ? { ...shellSeat(), count: 1 }
                      : v === "browser"
                        ? browserSeat()
                        : {
                            kind: "agent",
                            engineId: v,
                            role: s.kind === "agent" ? s.role : "Builder",
                            command: null,
                            url: null,
                          },
                  )
                }
              />
              {s.kind === "agent" ? (
                <>
                  <Combobox
                    className="min-w-0 flex-1"
                    ariaLabel={t("launch.role", { n })}
                    options={
                      s.role && !ROLES.includes(s.role as (typeof ROLES)[number])
                        ? [...roleOptions, { value: s.role, label: s.role }]
                        : roleOptions
                    }
                    value={s.role ?? ""}
                    onValueChange={(v) => set(i, { role: v })}
                    onCreate={(q) => set(i, { role: q.trim() })}
                    createLabel={(q) => t("launch.roleCreate", { role: q })}
                  />
                  <Input
                    type="number"
                    min={1}
                    max={MAX_COUNT}
                    hideLabel
                    label={t("launch.count", { n })}
                    className="w-16 shrink-0"
                    value={s.count}
                    onChange={(e) =>
                      set(i, { count: Math.max(1, Math.min(MAX_COUNT, Number(e.target.value) || 1)) })
                    }
                  />
                </>
              ) : s.kind === "browser" ? (
                <Input
                  hideLabel
                  monospace
                  label={t("launch.browserUrlField", { n })}
                  placeholder={t("launch.browserUrlPlaceholder")}
                  className="min-w-0 flex-1"
                  value={s.url ?? ""}
                  onChange={(e) => set(i, { url: e.target.value || null })}
                />
              ) : (
                <Input
                  hideLabel
                  monospace
                  label={t("launch.shellCommandField", { n })}
                  placeholder={t("launch.shellCommandPlaceholder")}
                  className="min-w-0 flex-1"
                  value={s.command ?? ""}
                  onChange={(e) => set(i, { command: e.target.value || null })}
                />
              )}
              <IconButton
                size="sm"
                icon={<X size={14} />}
                label={t("launch.removeSeat", { n })}
                disabled={seats.length === 1}
                onClick={() => onChange(seats.filter((_, k) => k !== i))}
              />
            </div>
            {s.kind === "agent" && s.engineId && (
              <SeatUsageHint engineId={s.engineId} accountId={s.accountId} workspaceId={workspaceId} />
            )}
          </li>
        );
      })}
    </ol>
  );
}

function LaunchSheetBody({ workspaceId }: { workspaceId: string }) {
  const ws = useWorkspaces((s) => s.list.find((w) => w.id === workspaceId));
  // A folder on a host lists the host's engines, not this computer's (F-RM-01).
  const remote = ws && ws.kind !== "local" ? ws.hostId : null;
  const localEngines = useEngines((s) => s.list);
  const hostEngines = useRemote((s) => (remote ? s.engines[remote] : undefined));
  const engines = useMemo(
    () => (remote ? (hostEngines ?? []) : localEngines),
    [remote, hostEngines, localEngines],
  );
  const presets = useLaunch((s) => s.presets);
  const { show } = useToast();

  const engine = defaultEngine(engines);
  // A host's worktrees are made by its git (D-1602); a WSL folder has none yet.
  const repo = isRepo(ws) && ws?.kind !== "wsl";
  const [presetId, setPresetId] = useState(SOLO);
  const [seats, setSeats] = useState<Seat[]>(() => [agentSeat(engine)]);
  const [isolation, setIsolation] = useState<Isolation>("shared");
  const [isolationTouched, setIsolationTouched] = useState(false);
  const [task, setTask] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<IpcError | null>(null);
  const [report, setReport] = useState<LaunchReport | null>(null);
  const [saving, setSaving] = useState(false);
  const close = () => useLaunch.getState().close();

  // A later detection (Check again, an install) can bring the first engine.
  useEffect(() => {
    if (engine)
      setSeats((list) =>
        list.map((s) => (s.kind === "agent" && !s.engineId ? { ...s, engineId: engine } : s)),
      );
  }, [engine]);

  const engineOptions: SelectOption[] = useMemo(
    () =>
      engines.map((e) => ({
        value: e.spec.id,
        label: e.install ? e.spec.label : t("launch.notInstalled", { label: e.spec.label }),
        disabled: e.install === null || e.unsupportedReason !== null,
      })),
    [engines],
  );
  const builtins = presets.filter((p) => p.builtin);
  const saved = presets.filter((p) => !p.builtin);
  const choose = (id: string) => {
    setPresetId(id);
    const p = presets.find((x) => x.id === id);
    if (!p) return;
    setSeats(lineupFrom(p, engine));
    setIsolation(repo ? p.isolation : "shared");
    setIsolationTouched(false);
  };
  const edit = (next: Seat[]) => {
    setSeats(next);
    setPresetId(CUSTOM);
    // More than one agent defaults to a worktree each (UI-12).
    if (!isolationTouched) setIsolation(repo && counts(next).agents > 1 ? "worktree" : "shared");
  };

  const c = counts(seats);
  const worktrees = isolation === "worktree" ? branchPreview(task, seats) : [];
  const ready =
    seats.length > 0 &&
    c.agents + c.shells + c.browsers > 0 &&
    seats.every((s) => (s.kind !== "agent" || s.engineId) && (s.kind !== "browser" || s.url?.trim()));
  const launch = async () => {
    if (!ready || busy || !ws) return;
    setBusy(true);
    setError(null);
    const out = await launchLineup(ws.id, seats, task, isolation);
    setBusy(false);
    if (!out.ok) return setError(out.error);
    if (out.value.launched.length > 0)
      show({ tone: "success", title: t("launch.launched", { n: out.value.launched.length }) });
    if (out.value.failed.length > 0) setReport(out.value);
    else close();
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      void launch();
    }
  };

  return (
    // Mod+Enter anywhere in the sheet launches (UI-12).
    <div data-testid="launch-sheet" className="flex flex-col gap-5" onKeyDown={onKeyDown}>
      <SegmentedControl
        size="sm"
        label={t("launch.preset")}
        value={presets.some((p) => p.id === presetId && p.builtin) ? presetId : CUSTOM}
        onValueChange={(id) => (id === CUSTOM ? setPresetId(CUSTOM) : choose(id))}
        items={[
          ...builtins.map((p) => ({ id: p.id, label: p.name })),
          { id: CUSTOM, label: t("launch.custom") },
        ]}
      />
      {saved.length > 0 && (
        <Select
          size="sm"
          label={t("launch.saved")}
          placeholder={t("launch.savedPlaceholder")}
          value={saved.some((p) => p.id === presetId) ? presetId : ""}
          onValueChange={choose}
          options={saved.map((p) => ({ value: p.id, label: p.name }))}
        />
      )}

      <section className="flex flex-col gap-2" aria-label={t("launch.lineup")}>
        <h3 className="text-sm font-semibold text-fg">{t("launch.lineup")}</h3>
        {engine === null && <Callout tone="warning">{t("launch.noEngines")}</Callout>}
        <Lineup seats={seats} onChange={edit} engineOptions={engineOptions} workspaceId={workspaceId} />
        <Menu
          trigger={
            <Button
              size="sm"
              variant="ghost"
              iconLeft={<Plus size={14} />}
              className="self-start"
              data-testid="launch-add-seat"
            >
              {t("launch.addSeat")}
            </Button>
          }
          entries={[
            {
              id: "agent",
              label: t("launch.addAgent"),
              onSelect: () => edit([...seats, agentSeat(engine, "Builder")]),
            },
            { id: "shell", label: t("launch.addShell"), onSelect: () => edit([...seats, shellSeat()]) },
            { id: "browser", label: t("launch.addBrowser"), onSelect: () => edit([...seats, browserSeat()]) },
          ]}
        />
      </section>

      <Textarea
        label={t("launch.task")}
        placeholder={t("launch.taskPlaceholder")}
        value={task}
        minRows={3}
        maxRows={8}
        onChange={(e) => setTask(e.target.value)}
        data-testid="launch-task"
      />

      <Checkbox
        label={t("launch.worktrees")}
        description={
          repo
            ? c.browsers > 0
              ? t("launch.worktreesOthersNote")
              : c.shells > 0
                ? t("launch.worktreesShellNote")
                : undefined
            : ws?.kind === "wsl"
              ? t("worktree.remote")
              : t("launch.needsRepo")
        }
        checked={isolation === "worktree"}
        disabled={!repo || c.agents === 0}
        onCheckedChange={(on) => {
          setIsolation(on ? "worktree" : "shared");
          setIsolationTouched(true);
        }}
      />

      <p data-testid="launch-summary" className="text-sm text-fg-muted">
        {t("launch.summary", {
          panes: c.agents + c.shells + c.browsers,
          worktrees: worktrees.length,
          branches: worktrees.join(", "),
        })}
      </p>

      {error && (
        <Callout tone="error" title={error.message}>
          {error.code === "limit_reached" ? t("launch.limitBody") : null}
        </Callout>
      )}
      {report && (
        <Failures
          report={report}
          workspaceId={workspaceId}
          task={task}
          isolation={isolation}
          onRetried={(i) => {
            const left = report.failed.filter((f) => f.seatIndex !== i);
            if (left.length === 0) close();
            else setReport({ ...report, failed: left });
          }}
        />
      )}

      {/* Wraps rather than pushing Launch off the sheet when labels run long
          (§5.5: found by the en-XA run). */}
      <div className="flex flex-wrap items-center justify-end gap-2 border-t border-divider pt-3">
        <Button
          variant="ghost"
          size="sm"
          className="mr-auto"
          onClick={() => setSaving(true)}
          disabled={!ready}
          data-testid="launch-save"
        >
          {t("launch.saveAs")}
        </Button>
        <Button variant="secondary" onClick={close}>
          {report ? t("common.close") : t("common.cancel")}
        </Button>
        {!report && (
          <Button
            variant="primary"
            loading={busy}
            disabled={!ready}
            kbd={shortcutLabel(["Mod", "Enter"])}
            onClick={() => void launch()}
            data-testid="launch-go"
          >
            {launchLabel(seats)}
          </Button>
        )}
      </div>
      {saving && <SaveDialog seats={seats} isolation={isolation} onClose={() => setSaving(false)} />}
    </div>
  );
}

/** UI-12: "Launch agents" (F-CD-14), from Mod+Shift+L, the empty state and
 * the palette. */
export function LaunchSheet() {
  const workspaceId = useLaunch((s) => s.workspaceId);
  const name = useWorkspaces((s) => s.list.find((w) => w.id === workspaceId)?.name ?? "");
  return (
    <SheetHost open={workspaceId !== null} subtitle={name}>
      {workspaceId && <LaunchSheetLoader key={workspaceId} workspaceId={workspaceId} />}
    </SheetHost>
  );
}

/** The lineup starts from the presets and the default engine, so the body
 * waits for both: after a restore, nothing else has loaded the engines. */
function LaunchSheetLoader({ workspaceId }: { workspaceId: string }) {
  const presetsLoaded = useLaunch((s) => s.loaded);
  const ws = useWorkspaces((s) => s.list.find((w) => w.id === workspaceId));
  const remote = ws && ws.kind !== "local" ? ws.hostId : null;
  const hostLoaded = useRemote((s) => (remote ? s.engines[remote] !== undefined : true));
  useEffect(() => {
    if (remote) void useRemote.getState().loadEngines(remote);
  }, [remote]);
  const enginesLoaded = useEngines((s) => s.loaded) && hostLoaded;
  useEffect(() => {
    if (!presetsLoaded) void useLaunch.getState().loadPresets();
  }, [presetsLoaded]);
  useEffect(() => {
    if (!useEngines.getState().loaded) void useEngines.getState().refresh();
  }, [enginesLoaded]);
  return (
    <SkeletonGate
      loading={!presetsLoaded || !enginesLoaded}
      label={t("launch.loading")}
      skeleton={
        <div className="flex flex-col gap-4">
          <Skeleton height={28} />
          <Skeleton height={28} />
          <Skeleton height={88} />
        </div>
      }
    >
      <LaunchSheetBody workspaceId={workspaceId} />
    </SkeletonGate>
  );
}

function SheetHost({ open, subtitle, children }: { open: boolean; subtitle: string; children: ReactNode }) {
  return (
    <Sheet
      open={open}
      onOpenChange={(o) => !o && useLaunch.getState().close()}
      title={t("launch.title")}
      subtitle={subtitle}
      closeLabel={t("common.close")}
    >
      {children}
    </Sheet>
  );
}
