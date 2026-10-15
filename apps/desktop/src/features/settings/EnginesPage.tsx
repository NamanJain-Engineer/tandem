import { useCallback, useEffect, useState } from "react";
import { ChevronDown, ChevronRight, RefreshCw } from "lucide-react";
import type {
  BackgroundProcess,
  EngineVersionView,
  EngineView,
  InstallMethod,
  IpcError,
  UpdateCheck,
} from "@tandem/protocol";
import {
  Badge,
  Banner,
  Button,
  ConfirmDialog,
  EngineGlyph,
  Input,
  KeyValue,
  Spinner,
  Switch,
  cx,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import { useWorkspaces } from "../../stores/workspaces";
import { useInstall } from "../code/InstallSheet";

/** Splits a shell-word list: spaces separate, single and double quotes group.
 * `null` when a quote is left open. */
export function parseArgs(text: string): string[] | null {
  const out: string[] = [];
  let cur = "";
  let quote: '"' | "'" | null = null;
  let has = false;
  for (const ch of text) {
    if (quote) {
      if (ch === quote) quote = null;
      else cur += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
      has = true;
    } else if (/\s/.test(ch)) {
      if (has || cur) out.push(cur);
      cur = "";
      has = false;
    } else {
      cur += ch;
    }
  }
  if (quote) return null;
  if (has || cur) out.push(cur);
  return out;
}

const quoteArg = (a: string) => (/^[A-Za-z0-9_\-./=:,@%+]+$/.test(a) ? a : `'${a.replace(/'/g, "'\\''")}'`);

/** How a control sequence reads ("Esc, Enter"). */
function keysOf(seq: string): string {
  const names: Record<string, string> = { "\u001b": "Esc", "\r": "Enter", "\n": "Ctrl+J", "\t": "Tab" };
  return [...seq].map((c) => names[c] ?? c).join(", ");
}

function ExtraArgs({ engine }: { engine: EngineView }) {
  const [draft, setDraft] = useState(engine.extraArgs.map(quoteArg).join(" "));
  const [error, setError] = useState<string | null>(null);
  const { show } = useToast();
  useEffect(() => setDraft(engine.extraArgs.map(quoteArg).join(" ")), [engine.extraArgs]);
  const save = async () => {
    const args = parseArgs(draft);
    if (!args) {
      setError(t("engines.args.unbalanced"));
      return;
    }
    setError(null);
    try {
      await invoke("engines_set_extra_args", { engineId: engine.spec.id, args });
      await useEngines.getState().refresh();
      show({ tone: "success", title: t("engines.args.saved") });
    } catch (e) {
      setError((e as { message?: string }).message ?? t("engines.args.unbalanced"));
    }
  };
  const id = `args-${engine.spec.id}`;
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-sm text-fg-muted">
        {t("engines.args")}
      </label>
      <Input
        id={id}
        data-testid={`engine-args-${engine.spec.id}`}
        value={draft}
        className="font-mono"
        placeholder={t("engines.args.placeholder")}
        onChange={(e) => setDraft(e.currentTarget.value)}
        onBlur={() => void save()}
        onKeyDown={(e) => e.key === "Enter" && void save()}
        aria-invalid={error ? true : undefined}
      />
      {error && (
        <p role="alert" className="text-sm text-error">
          {error}
        </p>
      )}
    </div>
  );
}

const METHOD: Record<
  InstallMethod,
  "engines.method.npm" | "engines.method.homebrew" | "engines.method.native" | "engines.method.other"
> = {
  npm: "engines.method.npm",
  homebrew: "engines.method.homebrew",
  native: "engines.method.native",
  other: "engines.method.other",
};

/** F-EN-08: Pin, Check for updates and Roll back for one installed engine. */
function Updates({
  engine,
  version,
  onChange,
  onRollback,
}: {
  engine: EngineView;
  version: EngineVersionView;
  onChange: () => void;
  onRollback: (to: string) => void;
}) {
  const [checking, setChecking] = useState(false);
  const [check, setCheck] = useState<UpdateCheck | null>(null);
  const [error, setError] = useState<string | null>(null);
  const id = engine.spec.id;
  const runCheck = async () => {
    setChecking(true);
    setError(null);
    try {
      setCheck(await invoke<UpdateCheck>("engine_check_update", { engineId: id }, { timeoutMs: 40_000 }));
    } catch (e) {
      setError((e as IpcError).message);
    } finally {
      setChecking(false);
    }
  };
  const pin = async (on: boolean) => {
    try {
      await invoke("engine_pin", { engineId: id, on });
      onChange();
    } catch (e) {
      setError((e as IpcError).message);
    }
  };
  const back = version.lastGood && version.lastGood !== version.version ? version.lastGood : version.previous;
  return (
    <div className="flex flex-col gap-2" data-testid={`engine-updates-${id}`}>
      {version.pinnable ? (
        <Switch
          checked={version.pinned}
          onCheckedChange={(on) => void pin(on)}
          label={t("engines.pin")}
          id={`engine-pin-${id}`}
        />
      ) : (
        <p className="text-sm text-fg-muted">{t("engines.noPin")}</p>
      )}
      <p className="text-xs text-fg-subtle">{t("engines.pinBody")}</p>
      <div className="flex flex-wrap items-center gap-2">
        <Button
          size="sm"
          variant="secondary"
          loading={checking}
          onClick={() => void runCheck()}
          data-testid={`engine-check-${id}`}
        >
          {t("engines.checkUpdates")}
        </Button>
        {back && (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => onRollback(back)}
            data-testid={`engine-rollback-${id}`}
          >
            {t("engines.rollbackTo", { version: back })}
          </Button>
        )}
        {check?.latest && (
          <span className="text-sm text-fg-muted" data-testid={`engine-latest-${id}`}>
            {check.latest === version.version
              ? t("engines.upToDate", { version: check.latest })
              : t("engines.newer", { version: check.latest })}
          </span>
        )}
        {check?.how && (
          <span className="text-sm text-fg-muted" data-testid={`engine-how-${id}`}>
            {t("engines.updateWith")} <code className="font-mono">{check.how}</code>
          </span>
        )}
      </div>
      <p className="text-xs text-fg-subtle">{t("engines.checkBody")}</p>
      {error && (
        <p role="alert" className="text-sm text-error">
          {error}
        </p>
      )}
    </div>
  );
}

function EngineRow({
  engine,
  version,
  onInstall,
  onChange,
  onRollback,
}: {
  engine: EngineView;
  version?: EngineVersionView;
  onInstall: () => void;
  onChange: () => void;
  onRollback: (to: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const detecting = useEngines((s) => s.detecting);
  const installed = engine.install !== null;
  return (
    <li data-testid={`engine-row-${engine.spec.id}`} className="border-b border-divider last:border-b-0">
      <div className="flex h-11 items-center gap-2 px-2">
        <button
          type="button"
          aria-expanded={open}
          aria-label={t("engines.details", { name: engine.spec.label })}
          onClick={() => setOpen((o) => !o)}
          className="inline-flex size-6 items-center justify-center rounded-sm text-fg-muted hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
        >
          {open ? (
            <ChevronDown size={14} aria-hidden="true" />
          ) : (
            <ChevronRight size={14} aria-hidden="true" />
          )}
        </button>
        <EngineGlyph letters={engine.spec.logo} size={20} />
        <span className="w-36 truncate text-base text-fg">{engine.spec.label}</span>
        <span className="flex flex-1 items-center gap-2 truncate text-sm text-fg-muted">
          {detecting ? (
            <Spinner />
          ) : installed ? (
            <>
              <span data-testid={`engine-version-${engine.spec.id}`}>
                {engine.install?.version ?? t("engines.versionUnknown")}
                {version && ` · ${t(METHOD[version.method])}`}
              </span>
              {version?.selfUpdates && !version.pinned && (
                <Badge variant="outline">{t("engines.updatesItself")}</Badge>
              )}
              {engine.install?.source === "known" && (
                <Badge variant="waiting">{t("engines.notOnPath")}</Badge>
              )}
              <span>· {engine.chatCapable ? t("engines.thread") : t("engines.terminalOnly")}</span>
            </>
          ) : (
            t("engines.notInstalled")
          )}
        </span>
        {!installed && (
          <Button
            size="sm"
            variant="secondary"
            onClick={onInstall}
            data-testid={`engine-install-${engine.spec.id}`}
          >
            {t("engines.install")}
          </Button>
        )}
      </div>
      {/* Who can use it (F-EN-09), from the registry. */}
      {engine.spec.note && (
        <p className="px-10 pb-2 text-sm text-fg-muted" data-testid={`engine-note-${engine.spec.id}`}>
          {engine.spec.note}
        </p>
      )}
      {open && (
        <div className="flex flex-col gap-3 px-10 pb-3">
          <KeyValue
            items={[
              {
                key: t("engines.path"),
                value: <span className="font-mono">{engine.install?.path ?? "—"}</span>,
              },
              { key: t("engines.version"), value: engine.install?.version ?? "—" },
              { key: t("engines.newline"), value: keysOf(engine.spec.newlineSequence) },
              { key: t("engines.vendor"), value: engine.spec.vendor },
            ]}
          />
          <ExtraArgs engine={engine} />
          {version && (
            <Updates engine={engine} version={version} onChange={onChange} onRollback={onRollback} />
          )}
        </div>
      )}
    </li>
  );
}

/** UI-33 / UI-105: the engines Tandem found, with versions, Install, the
 * detection details and the user's extra launch arguments. */
export function EnginesPage() {
  const list = useEngines((s) => s.list);
  const loaded = useEngines((s) => s.loaded);
  const report = useEngines((s) => s.report);
  const detecting = useEngines((s) => s.detecting);
  const [details, setDetails] = useState(false);
  const tabId = useWorkspaces((s) => (s.activeId ? s.activeTab[s.activeId] : undefined));
  useEffect(() => {
    if (!loaded) void useEngines.getState().refresh();
  }, [loaded]);
  const sorted = [...list].sort((a, b) => Number(b.install !== null) - Number(a.install !== null));
  // F-EN-08: versions, pins and background processes; read on this computer.
  const [versions, setVersions] = useState<EngineVersionView[]>([]);
  const [background, setBackground] = useState<BackgroundProcess[] | null>(null);
  const [backgroundError, setBackgroundError] = useState<string | null>(null);
  const [stopping, setStopping] = useState<BackgroundProcess | null>(null);
  const { show } = useToast();
  const loadVersions = useCallback(() => {
    void invoke<EngineVersionView[] | null>("engine_versions", {}).then(
      (v) => setVersions(v ?? []),
      () => setVersions([]),
    );
    void invoke<BackgroundProcess[] | null>("engine_background", {}).then(
      (b) => {
        setBackground(b ?? []);
        setBackgroundError(null);
      },
      (e: IpcError) => setBackgroundError(e.message),
    );
  }, []);
  useEffect(() => {
    if (loaded) loadVersions();
  }, [loaded, list, loadVersions]);
  const label = (id: string) => list.find((e) => e.spec.id === id)?.spec.label ?? id;
  const rollback = (id: string, to: string) => {
    const engine = list.find((e) => e.spec.id === id);
    if (engine) useInstall.getState().open(engine, tabId ?? null, to);
  };
  return (
    <div className="flex flex-col gap-4" data-testid="engines-page">
      {versions
        .filter((v) => v.broken)
        .map((v) => (
          <Banner
            key={v.engineId}
            tone="warning"
            action={{
              label: t("engines.rollbackTo", { version: v.broken?.from ?? "" }),
              onClick: () => rollback(v.engineId, v.broken?.from ?? ""),
            }}
          >
            <span data-testid={`engine-broken-${v.engineId}`}>
              {t("engines.broken", {
                name: label(v.engineId),
                from: v.broken?.from ?? "",
                to: v.broken?.to ?? "",
              })}
            </span>
          </Banner>
        ))}
      <div className="flex items-center justify-between gap-3">
        <p className="text-sm text-fg-muted">{t("engines.pageBody")}</p>
        <Button
          size="sm"
          variant="secondary"
          disabled={detecting}
          onClick={() => void useEngines.getState().detect()}
          data-testid="engines-rescan"
        >
          {detecting ? <Spinner /> : <RefreshCw size={14} aria-hidden="true" />}
          {t("engines.checkAgain")}
        </Button>
      </div>
      {!loaded ? (
        <Spinner />
      ) : (
        <ul className="rounded-md border border-border bg-surface-2">
          {sorted.map((e) => (
            <EngineRow
              key={e.spec.id}
              engine={e}
              version={versions.find((v) => v.engineId === e.spec.id)}
              onInstall={() => useInstall.getState().open(e, tabId ?? null)}
              onChange={loadVersions}
              onRollback={(to) => rollback(e.spec.id, to)}
            />
          ))}
        </ul>
      )}
      <section className="flex flex-col gap-1.5" data-testid="engines-background">
        <h3 className="text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
          {t("engines.background")}
        </h3>
        {backgroundError ? (
          <p className="text-sm text-fg-muted">{backgroundError}</p>
        ) : background === null ? (
          <Spinner />
        ) : background.length === 0 ? (
          <p className="text-sm text-fg-muted" data-testid="engines-background-empty">
            {t("engines.backgroundEmpty")}
          </p>
        ) : (
          <ul className="rounded-md border border-border bg-surface-2">
            {background.map((b) => (
              <li
                key={b.pid}
                data-testid="engine-background-row"
                data-pid={b.pid}
                className="flex items-center gap-2 border-b border-divider px-3 py-2 last:border-b-0"
              >
                <span className="w-32 truncate text-sm text-fg">{label(b.engineId)}</span>
                <span className="min-w-0 flex-1 truncate font-mono text-xs text-fg-muted" title={b.command}>
                  {b.command}
                </span>
                <Button size="sm" variant="secondary" onClick={() => setStopping(b)}>
                  {t("engines.stop")}
                </Button>
              </li>
            ))}
          </ul>
        )}
      </section>
      <ConfirmDialog
        open={stopping !== null}
        onOpenChange={(o) => !o && setStopping(null)}
        danger
        title={t("engines.stopTitle", { name: stopping ? label(stopping.engineId) : "" })}
        body={t("engines.stopBody", { pid: stopping?.pid ?? 0 })}
        confirmLabel={t("engines.stop")}
        onConfirm={() => {
          const b = stopping;
          setStopping(null);
          if (b)
            void invoke("engine_background_stop", { pid: b.pid }).then(
              () => {
                show({ tone: "success", title: t("engines.stopped", { name: label(b.engineId) }) });
                loadVersions();
              },
              (e: IpcError) => show({ tone: "error", title: e.message }),
            );
        }}
      />
      {report && (
        <div className="flex flex-col gap-2 text-sm">
          <div className="flex items-center gap-2 text-fg-muted">
            <span data-testid="engines-detection">
              {t("engines.detectionSummary", {
                shells: report.shellsAsked.join(", "),
                n: report.foldersChecked.length,
              })}
            </span>
            <Button size="sm" variant="ghost" onClick={() => setDetails((d) => !d)} aria-expanded={details}>
              {details ? t("engines.hideDetails") : t("engines.showDetails")}
            </Button>
          </div>
          {report.slowShell && (
            <p className="text-warning-text">
              {t("engines.slowShell", { seconds: report.slowShell.seconds })}
            </p>
          )}
          {details && (
            <div
              className={cx(
                "flex flex-col gap-2 rounded-md border border-border bg-surface-1 p-3 font-mono text-xs text-fg-muted",
              )}
            >
              {report.foldersChecked.map((f) => (
                <span key={f}>{f}</span>
              ))}
              {report.fixes.map((f) => (
                <span key={f} className="font-sans text-sm text-fg">
                  {f}
                </span>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
