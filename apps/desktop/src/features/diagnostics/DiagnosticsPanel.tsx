// Help › Diagnostics (PRODUCT_SPEC §5.7): a live, local-only view of the
// app's subsystems (the watchdog's health, §5.2), each terminal's spawn time
// and output queue, the event bus's queues and the window's frame rate.
// Polls `app_diagnostics` once a second while open; nothing leaves the machine.
import { useEffect, useState } from "react";
import type { Diagnostics, IpcError, SubsystemHealth } from "@tandem/protocol";
import { Badge, Button, Dialog, Skeleton, SkeletonGate, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useUi } from "../../stores/ui";
import { copyDiagnostics } from "../shell/actions";

export const REFRESH_MS = 1000;

/** Frames per second over the last second, from requestAnimationFrame. */
function useFps(active: boolean): number | null {
  const [fps, setFps] = useState<number | null>(null);
  useEffect(() => {
    if (!active || typeof requestAnimationFrame !== "function") return;
    let frames = 0;
    let start = performance.now();
    let raf = requestAnimationFrame(function tick(now) {
      frames += 1;
      if (now - start >= 1000) {
        setFps(Math.round((frames * 1000) / (now - start)));
        frames = 0;
        start = now;
      }
      raf = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(raf);
  }, [active]);
  return fps;
}

const TONE: Record<SubsystemHealth["state"], "done" | "waiting" | "error" | "idle"> = {
  running: "done",
  restarting: "waiting",
  failed: "error",
  stopped: "idle",
};

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="text-sm font-semibold text-fg">{title}</h3>
      {children}
    </section>
  );
}

export function DiagnosticsPanel() {
  const open = useUi((s) => s.diagnosticsOpen);
  const { show } = useToast();
  const [data, setData] = useState<Diagnostics | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fps = useFps(open);

  useEffect(() => {
    if (!open) {
      setData(null);
      setError(null);
      return;
    }
    const ctl = new AbortController();
    const read = () =>
      invoke<Diagnostics>("app_diagnostics", undefined, { signal: ctl.signal })
        .then((d) => {
          setData(d);
          setError(null);
        })
        .catch((e: IpcError) => {
          if (e.code !== "cancelled") setError(e.message);
        });
    void read();
    const timer = setInterval(() => void read(), REFRESH_MS);
    return () => {
      ctl.abort();
      clearInterval(timer);
    };
  }, [open]);

  const minutes = data ? Math.floor(Number(data.uptimeMs) / 60_000) : 0;
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => useUi.getState().setDiagnostics(o)}
      title={t("diag.title")}
      description={t("diag.note")}
      size="lg"
      closeLabel={t("common.close")}
      footer={
        <Button variant="secondary" onClick={() => void copyDiagnostics(show)}>
          {t("menu.copyDiagnostics")}
        </Button>
      }
    >
      {error && !data ? (
        <p role="alert" className="text-sm text-danger">
          {t("diag.error", { message: error })}
        </p>
      ) : (
        <SkeletonGate
          loading={!data}
          label={t("diag.loading")}
          skeleton={
            <div className="flex flex-col gap-2">
              <Skeleton width="40%" />
              <Skeleton width="70%" />
              <Skeleton width="55%" />
            </div>
          }
        >
          {data && (
            // Its own scroll region, reachable by keyboard (axe:
            // scrollable-region-focusable), so the dialog body never scrolls.
            <div
              className="flex max-h-[60vh] flex-col gap-5 overflow-y-auto pr-1 text-sm focus-visible:shadow-focus focus-visible:outline-none"
              data-testid="diagnostics"
              role="region"
              aria-label={t("diag.title")}
              tabIndex={0}
            >
              <p className="text-fg-muted">
                {t("diag.uptime", { minutes })} · <span data-testid="diag-fps">{fps === null ? t("diag.fpsMeasuring") : t("diag.fps", { fps })}</span>
              </p>
              <Section title={t("diag.subsystems")}>
                <ul className="flex flex-col divide-y divide-divider">
                  {data.subsystems.map((s) => (
                    <li key={s.name} className="flex flex-wrap items-center gap-x-3 gap-y-1 py-1.5">
                      <span className="font-mono text-fg">{s.name}</span>
                      <Badge variant={TONE[s.state]}>{t(`diag.state.${s.state}`)}</Badge>
                      {s.sinceBeatMs !== null && (
                        <span className="text-fg-muted">{t("diag.lastBeat", { seconds: Math.round(Number(s.sinceBeatMs) / 100) / 10 })}</span>
                      )}
                      <span className="text-fg-muted">{t("diag.restarts", { count: s.restarts })}</span>
                      {s.lastError && <span className="text-fg-subtle">{s.lastError}</span>}
                    </li>
                  ))}
                </ul>
              </Section>
              <Section title={t("diag.terminals")}>
                {data.panes.length === 0 ? (
                  <p className="text-fg-muted">{t("diag.noTerminals")}</p>
                ) : (
                  <ul className="flex flex-col divide-y divide-divider">
                    {data.panes.map((p) => (
                      <li key={p.paneId} className="flex flex-wrap gap-x-3 py-1.5">
                        <span className="font-mono text-fg">{p.paneId}</span>
                        <span className="text-fg-muted">{t("diag.spawn", { ms: Number(p.spawnMs) })}</span>
                        <span className="text-fg-muted">{t("diag.queued", { bytes: Number(p.queuedBytes) })}</span>
                        {p.exited && <Badge variant="neutral">{t("diag.exited")}</Badge>}
                      </li>
                    ))}
                  </ul>
                )}
              </Section>
              <Section title={t("diag.bus")}>
                <ul className="grid grid-cols-2 gap-x-6 gap-y-1 sm:grid-cols-3">
                  {data.bus.map((b) => (
                    <li key={b.topic} data-testid={`diag-bus-${b.topic}`} className="flex justify-between gap-2">
                      <span className="font-mono text-fg-muted">{b.topic}</span>
                      <span className="tabular-nums text-fg">{b.queued}</span>
                    </li>
                  ))}
                </ul>
              </Section>
            </div>
          )}
        </SkeletonGate>
      )}
    </Dialog>
  );
}
