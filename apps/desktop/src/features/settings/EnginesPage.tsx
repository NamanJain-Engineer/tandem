import { useEffect, useState } from "react";
import { ChevronDown, ChevronRight, RefreshCw } from "lucide-react";
import type { EngineView } from "@tandem/protocol";
import { Badge, Button, EngineGlyph, Input, KeyValue, Spinner, cx, useToast } from "@tandem/ui";
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

function EngineRow({ engine, onInstall }: { engine: EngineView; onInstall: () => void }) {
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
          {open ? <ChevronDown size={14} aria-hidden="true" /> : <ChevronRight size={14} aria-hidden="true" />}
        </button>
        <EngineGlyph letters={engine.spec.logo} size={20} />
        <span className="w-36 truncate text-base text-fg">{engine.spec.label}</span>
        <span className="flex flex-1 items-center gap-2 truncate text-sm text-fg-muted">
          {detecting ? (
            <Spinner />
          ) : installed ? (
            <>
              <span data-testid={`engine-version-${engine.spec.id}`}>{engine.install?.version ?? t("engines.versionUnknown")}</span>
              {engine.install?.source === "known" && <Badge variant="waiting">{t("engines.notOnPath")}</Badge>}
              <span>· {engine.chatCapable ? t("engines.thread") : t("engines.terminalOnly")}</span>
            </>
          ) : (
            t("engines.notInstalled")
          )}
        </span>
        {!installed && (
          <Button size="sm" variant="secondary" onClick={onInstall} data-testid={`engine-install-${engine.spec.id}`}>
            {t("engines.install")}
          </Button>
        )}
      </div>
      {open && (
        <div className="flex flex-col gap-3 px-10 pb-3">
          <KeyValue
            items={[
              { key: t("engines.path"), value: <span className="font-mono">{engine.install?.path ?? "—"}</span> },
              { key: t("engines.version"), value: engine.install?.version ?? "—" },
              { key: t("engines.newline"), value: keysOf(engine.spec.newlineSequence) },
              { key: t("engines.vendor"), value: engine.spec.vendor },
            ]}
          />
          <ExtraArgs engine={engine} />
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
  return (
    <div className="flex flex-col gap-4" data-testid="engines-page">
      <div className="flex items-center justify-between gap-3">
        <p className="text-sm text-fg-muted">{t("engines.pageBody")}</p>
        <Button size="sm" variant="secondary" disabled={detecting} onClick={() => void useEngines.getState().detect()} data-testid="engines-rescan">
          {detecting ? <Spinner /> : <RefreshCw size={14} aria-hidden="true" />}
          {t("engines.checkAgain")}
        </Button>
      </div>
      {!loaded ? (
        <Spinner />
      ) : (
        <ul className="rounded-md border border-border bg-surface-2">
          {sorted.map((e) => (
            <EngineRow key={e.spec.id} engine={e} onInstall={() => useInstall.getState().open(e, tabId ?? null)} />
          ))}
        </ul>
      )}
      {report && (
        <div className="flex flex-col gap-2 text-sm">
          <div className="flex items-center gap-2 text-fg-muted">
            <span data-testid="engines-detection">
              {t("engines.detectionSummary", { shells: report.shellsAsked.join(", "), n: report.foldersChecked.length })}
            </span>
            <Button size="sm" variant="ghost" onClick={() => setDetails((d) => !d)} aria-expanded={details}>
              {details ? t("engines.hideDetails") : t("engines.showDetails")}
            </Button>
          </div>
          {report.slowShell && <p className="text-warning-text">{t("engines.slowShell", { seconds: report.slowShell.seconds })}</p>}
          {details && (
            <div className={cx("flex flex-col gap-2 rounded-md border border-border bg-surface-1 p-3 font-mono text-xs text-fg-muted")}>
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
