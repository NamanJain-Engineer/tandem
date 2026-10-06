import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { Check, Copy, SquareTerminal } from "lucide-react";
import type { EngineView, InstallCommand, PtyMark } from "@tandem/protocol";
import { Button, Callout, EngineGlyph, Sheet, Spinner, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { copyText } from "../../ipc/clipboard";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import { useLayout } from "../../stores/layout";
import { useUi } from "../../stores/ui";
import { switchMode } from "../shell/actions";

type Step = { kind: "ready" } | { kind: "running"; paneId: string } | { kind: "installed"; version: string | null } | { kind: "missing" };

/** How long the new shell gets to show its prompt before the command is typed anyway. */
const PROMPT_WAIT_MS = 3_000;

type InstallState = {
  engine: EngineView | null;
  /** The tab the installer's terminal opens in. */
  tabId: string | null;
  open: (engine: EngineView, tabId: string | null) => void;
  close: () => void;
};

/** One install sheet for the whole app: it outlives the launch grid that
 * opened it (the installer's pane replaces an empty tab's grid). */
export const useInstall = create<InstallState>((set) => ({
  engine: null,
  tabId: null,
  open: (engine, tabId) => set({ engine, tabId }),
  close: () => set({ engine: null, tabId: null }),
}));

export function InstallHost() {
  const engine = useInstall((s) => s.engine);
  const tabId = useInstall((s) => s.tabId);
  return <InstallSheet engine={engine} tabId={tabId} onClose={() => useInstall.getState().close()} />;
}

/** UI-34 / F-EN-03: the registry's install command, exactly as listed, with
 * Copy and Run in terminal. Run types it into a visible terminal pane (never
 * hidden, never with sudo added); when it finishes, Tandem detects again. */
export function InstallSheet({ engine, tabId, onClose }: { engine: EngineView | null; tabId: string | null; onClose: () => void }) {
  const [command, setCommand] = useState<InstallCommand | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [step, setStep] = useState<Step>({ kind: "ready" });
  const [copied, setCopied] = useState(false);
  const { show } = useToast();
  const engineId = engine?.spec.id ?? null;
  const off = useRef<Array<() => void>>([]);

  useEffect(() => {
    setCommand(null);
    setError(null);
    setStep({ kind: "ready" });
    setCopied(false);
    if (!engineId) return;
    invoke<InstallCommand>("engines_install_command", { engineId })
      .then(setCommand)
      .catch((e: { message?: string }) => setError(e.message ?? t("install.none")));
  }, [engineId]);

  useEffect(
    () => () => {
      off.current.forEach((f) => f());
      off.current = [];
    },
    [engineId],
  );

  const recheck = async () => {
    const report = await useEngines.getState().detect();
    const found = report?.found.find((f) => f.engineId === engineId);
    setStep(found ? { kind: "installed", version: found.version } : { kind: "missing" });
  };

  const run = async () => {
    if (!command || !tabId) return;
    // The installer runs where it can be seen (from Settings too).
    if (useUi.getState().settingsOpen) useUi.getState().closeSettings();
    switchMode("code");
    const res = await useLayout.getState().split(tabId, null, "right", { kind: "terminal" });
    if (!res.ok) {
      show({ tone: "error", title: res.error.message });
      return;
    }
    const paneId = res.pane.id;
    setStep({ kind: "running", paneId });
    let typed = false;
    const type = () => {
      if (typed) return;
      typed = true;
      void invoke("pty_write", { paneId, data: `${command.display}\r` }).catch(() => undefined);
    };
    const timer = setTimeout(type, PROMPT_WAIT_MS);
    // The shell's first prompt (OSC 133 A) says it's ready; the command's end
    // (OSC 133 D) says the installer finished.
    const stop = await listen<PtyMark>("pty:mark", (mark) => {
      if (mark.paneId !== paneId) return;
      if (mark.kind === "A" && !typed) {
        clearTimeout(timer);
        type();
      } else if (mark.kind === "D" && typed) {
        stop();
        void recheck();
      }
    });
    off.current.push(stop, () => clearTimeout(timer));
  };

  const copy = async () => {
    if (command && (await copyText(command.display))) {
      setCopied(true);
      setTimeout(() => setCopied(false), 1_500);
    }
  };

  const label = engine?.spec.label ?? "";
  return (
    <Sheet open={engine !== null} onOpenChange={(o) => !o && onClose()} title={t("install.title", { name: label })} subtitle={engine?.spec.vendor} closeLabel={t("common.close")}>
      <div data-testid="install-sheet" className="flex flex-col gap-4">
        <div className="flex items-center gap-2">
          <EngineGlyph letters={engine?.spec.logo ?? ""} size={20} />
          <p className="text-sm text-fg-muted">{t("install.body", { name: label })}</p>
        </div>
        {error && <Callout tone="error" title={error} />}
        {command && (
          <pre data-testid="install-command" className="whitespace-pre-wrap break-all rounded-md border border-border bg-surface-1 p-3 font-mono text-sm text-fg">
            {command.display}
          </pre>
        )}
        {step.kind === "ready" && command && (
          <div className="flex gap-2">
            {/* Without an open workspace there's no terminal to run it in; Copy still works. */}
            <Button variant="primary" disabled={!tabId} onClick={() => void run()} data-testid="install-run">
              <SquareTerminal size={16} aria-hidden="true" />
              {t("install.run")}
            </Button>
            <Button variant="secondary" onClick={() => void copy()}>
              {copied ? <Check size={16} aria-hidden="true" /> : <Copy size={16} aria-hidden="true" />}
              {copied ? t("install.copied") : t("install.copy")}
            </Button>
          </div>
        )}
        {step.kind === "running" && (
          <p className="flex items-center gap-2 text-sm text-fg-muted" data-testid="install-running">
            <Spinner />
            {t("install.running")}
          </p>
        )}
        {step.kind === "installed" && (
          <Callout tone="success" title={t("install.done", { name: label })}>
            <p data-testid="install-done">{step.version ? t("install.doneVersion", { name: label, version: step.version }) : t("install.doneBody")}</p>
          </Callout>
        )}
        {step.kind === "missing" && (
          <Callout
            tone="warning"
            title={t("install.notFound", { name: label })}
            actions={
              <Button size="sm" variant="secondary" onClick={() => void recheck()}>
                {t("engines.checkAgain")}
              </Button>
            }
          />
        )}
        <p className="text-xs text-fg-subtle">{t("install.never")}</p>
      </div>
    </Sheet>
  );
}
