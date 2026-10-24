import { useEffect, useState } from "react";
import { ChevronDown, ChevronRight, Globe, RefreshCw, Smartphone, SquareTerminal } from "lucide-react";
import type { EngineView } from "@tandem/protocol";
import { Button, EngineGlyph, Spinner, cx } from "@tandem/ui";
import { t } from "../../i18n";
import { useEngines } from "../../stores/engines";
import { useRemote } from "../../stores/remote";
import { useInstall } from "./InstallSheet";
import { invoke } from "../../ipc/invoke";

const TILE = cx(
  "flex h-14 w-full items-center gap-3 rounded-md border border-border bg-surface-2 px-3 text-left",
  "hover:border-border-strong hover:bg-surface-3 focus-visible:shadow-focus focus-visible:outline-none disabled:opacity-60",
);

/** A remote workspace's + menu: the host's own engines (F-RM-01), the
 * Terminal tile and the Browser; Install is the host's business. */
function RemoteLaunchGrid({
  hostId,
  onPick,
  busy,
  autoFocus,
  browser,
}: {
  hostId: string;
  onPick: (tileId: string) => void;
  busy: string | null;
  autoFocus?: boolean;
  browser?: boolean;
}) {
  const list = useRemote((s) => s.engines[hostId]);
  const host = useRemote((s) => s.hosts.find((h) => h.id === hostId));
  const [checking, setChecking] = useState(false);
  useEffect(() => {
    void useRemote.getState().loadEngines(hostId);
  }, [hostId]);
  const check = async () => {
    setChecking(true);
    await invoke("engines_detect", { hostId }, { timeoutMs: 60_000 }).catch(() => undefined);
    await useRemote.getState().loadEngines(hostId);
    setChecking(false);
  };
  if (!list) return <Spinner />;
  const installed = list.filter((e) => e.install);
  // "Claude Code (in Ubuntu)" for a WSL distro, "On <host>" for SSH.
  const where =
    host?.kind === "wsl"
      ? t("picker.inDistro", { distro: host.wslDistro ?? host.label })
      : t("picker.onHost", { host: host?.label ?? "" });
  const tile = (id: string, label: string, glyph: React.ReactNode, detail: string | null, first: boolean) => (
    <li key={id}>
      <button
        type="button"
        data-testid={`tile-${id}`}
        autoFocus={autoFocus && first}
        disabled={busy !== null}
        onClick={() => onPick(id)}
        className={TILE}
      >
        {busy === id ? <Spinner /> : glyph}
        <span className="flex min-w-0 flex-col">
          <span className="truncate text-base font-medium text-fg">{label}</span>
          {detail && <span className="truncate text-xs text-fg-subtle">{detail}</span>}
        </span>
      </button>
    </li>
  );
  return (
    <div className="flex flex-col gap-3" data-testid="launch-grid" data-host={hostId}>
      <ul className="grid grid-cols-2 gap-2">
        {installed.map((e, i) =>
          tile(e.spec.id, e.spec.label, <EngineGlyph letters={e.spec.logo} engine={e.spec.id} size={20} />, where, i === 0),
        )}
        {tile(
          "shell",
          t("picker.terminal"),
          <SquareTerminal size={20} className="text-fg-muted" aria-hidden="true" />,
          null,
          installed.length === 0,
        )}
        {browser &&
          tile(
            "browser",
            t("picker.browser"),
            <Globe size={20} className="text-fg-muted" aria-hidden="true" />,
            // It browses from here: the host's own dev servers aren't reachable.
            t("picker.browserHere"),
            false,
          )}
      </ul>
      {installed.length === 0 && (
        <div
          data-testid="engines-diagnostics"
          className="flex flex-col gap-2 rounded-md border border-border bg-surface-2 p-3"
        >
          <p className="text-sm text-fg">
            {t("picker.noneOnHost", {
              host: host?.kind === "wsl" ? (host.wslDistro ?? host.label) : (host?.label ?? ""),
            })}
          </p>
          <div>
            <Button
              size="sm"
              variant="secondary"
              disabled={checking}
              onClick={() => void check()}
              data-testid="check-again"
            >
              {checking ? <Spinner /> : <RefreshCw size={14} aria-hidden="true" />}
              {t("engines.checkAgain")}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

/** Loads the engine list once, and keeps it fresh while shown. */
export function useEngineList(): { installed: EngineView[]; missing: EngineView[]; loaded: boolean } {
  const list = useEngines((s) => s.list);
  const loaded = useEngines((s) => s.loaded);
  useEffect(() => {
    if (!loaded) void useEngines.getState().refresh();
  }, [loaded]);
  return { installed: list.filter((e) => e.install), missing: list.filter((e) => !e.install), loaded };
}

/** F-CD-13 / UI-10: installed engines first as 2-column tiles, the Terminal
 * tile, "N more not installed" with Install, and — when nothing was found —
 * what was probed, the likely fix and Check again. */
export function LaunchGrid({
  tabId,
  onPick,
  busy,
  autoFocus,
  browser,
  simulator,
  hostId,
}: {
  tabId: string;
  onPick: (tileId: string) => void;
  busy: string | null;
  autoFocus?: boolean;
  /** The pane picker also offers a Browser pane (F-CD-17). */
  browser?: boolean;
  /** …and, on a Mac with an iOS Simulator runtime, a Simulator pane (F-CD-21). */
  simulator?: boolean;
  /** A remote workspace's host: its engines instead of this machine's. */
  hostId?: string | null;
}) {
  if (hostId && hostId !== "local") {
    return (
      <RemoteLaunchGrid hostId={hostId} onPick={onPick} busy={busy} autoFocus={autoFocus} browser={browser} />
    );
  }
  return (
    <LocalLaunchGrid
      tabId={tabId}
      onPick={onPick}
      busy={busy}
      autoFocus={autoFocus}
      browser={browser}
      simulator={simulator}
    />
  );
}

function LocalLaunchGrid({
  tabId,
  onPick,
  busy,
  autoFocus,
  browser,
  simulator,
}: {
  tabId: string;
  onPick: (tileId: string) => void;
  busy: string | null;
  autoFocus?: boolean;
  browser?: boolean;
  simulator?: boolean;
}) {
  const { installed, missing, loaded } = useEngineList();
  const report = useEngines((s) => s.report);
  const detecting = useEngines((s) => s.detecting);
  const [more, setMore] = useState(false);
  const tile = (id: string, label: string, glyph: React.ReactNode, detail: string | null, first: boolean) => (
    <li key={id}>
      <button
        type="button"
        data-testid={`tile-${id}`}
        autoFocus={autoFocus && first}
        disabled={busy !== null}
        onClick={() => onPick(id)}
        className={TILE}
      >
        {busy === id ? <Spinner /> : glyph}
        <span className="flex min-w-0 flex-col">
          <span className="truncate text-base font-medium text-fg">{label}</span>
          {detail && <span className="truncate text-xs text-fg-subtle">{detail}</span>}
        </span>
      </button>
    </li>
  );

  if (!loaded) return <Spinner />;
  return (
    <div className="flex flex-col gap-3" data-testid="launch-grid">
      <ul className="grid grid-cols-2 gap-2">
        {installed.map((e, i) =>
          tile(
            e.spec.id,
            e.spec.label,
            <EngineGlyph letters={e.spec.logo} engine={e.spec.id} size={20} />,
            e.install?.version ?? null,
            i === 0,
          ),
        )}
        {tile(
          "shell",
          t("picker.terminal"),
          <SquareTerminal size={20} className="text-fg-muted" aria-hidden="true" />,
          null,
          installed.length === 0,
        )}
        {browser &&
          tile(
            "browser",
            t("picker.browser"),
            <Globe size={20} className="text-fg-muted" aria-hidden="true" />,
            null,
            false,
          )}
        {simulator &&
          tile(
            "simulator",
            t("picker.simulator"),
            <Smartphone size={20} className="text-fg-muted" aria-hidden="true" />,
            null,
            false,
          )}
      </ul>
      {report?.slowShell && (
        // Known folders were still checked: what's above is real (F-EN-01 AC2).
        <p className="text-sm text-warning-text" data-testid="slow-shell">
          {t("engines.slowShell", { seconds: report.slowShell.seconds })}
        </p>
      )}
      {installed.length === 0 && (
        <div
          data-testid="engines-diagnostics"
          className="flex flex-col gap-2 rounded-md border border-border bg-surface-2 p-3"
        >
          <p className="text-sm text-fg">{t("engines.noneFound")}</p>
          {report && (
            <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
              <dt className="text-fg-subtle">{t("engines.shellsAsked")}</dt>
              <dd className="font-mono text-fg-muted" data-testid="shells-asked">
                {report.shellsAsked.join(", ")}
              </dd>
              <dt className="text-fg-subtle">{t("engines.foldersChecked")}</dt>
              <dd className="font-mono text-fg-muted">{report.foldersChecked.join(", ")}</dd>
            </dl>
          )}
          <div>
            <Button
              size="sm"
              variant="secondary"
              disabled={detecting}
              onClick={() => void useEngines.getState().detect()}
              data-testid="check-again"
            >
              {detecting ? <Spinner /> : <RefreshCw size={14} aria-hidden="true" />}
              {t("engines.checkAgain")}
            </Button>
          </div>
        </div>
      )}
      {missing.length > 0 && (
        <div className="flex flex-col gap-1">
          <button
            type="button"
            data-testid="more-engines"
            aria-expanded={more}
            onClick={() => setMore((m) => !m)}
            className="inline-flex items-center gap-1 self-start rounded-sm text-sm text-fg-muted hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
          >
            {more ? (
              <ChevronDown size={14} aria-hidden="true" />
            ) : (
              <ChevronRight size={14} aria-hidden="true" />
            )}
            {t("engines.moreNotInstalled", { n: missing.length })}
          </button>
          {more && (
            <ul className="flex flex-col">
              {missing.map((e) => (
                <li
                  key={e.spec.id}
                  data-testid={`missing-${e.spec.id}`}
                  className="flex min-h-9 items-center gap-2 border-b border-divider px-1 py-1 last:border-b-0"
                >
                  <EngineGlyph letters={e.spec.logo} engine={e.spec.id} size={16} />
                  <span className="flex min-w-0 flex-1 flex-col">
                    <span className="truncate text-sm text-fg">{e.spec.label}</span>
                    {/* Who can use it (F-EN-09), from the registry. */}
                    {e.spec.note && (
                      <span className="text-xs text-fg-muted" data-testid={`engine-note-${e.spec.id}`}>
                        {e.spec.note}
                      </span>
                    )}
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => useInstall.getState().open(e, tabId)}
                    data-testid={`install-${e.spec.id}`}
                  >
                    {t("engines.install")}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
