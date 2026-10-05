import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import type { ExitInfo, IpcError, PtyInfo, AttachSnapshot } from "@tandem/protocol";
import { Button, Spinner, type TerminalTheme } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { log } from "../../ipc/log";
import { createFrameChannel, FrameKind, type Frame, type FrameChannel } from "../../ipc/pty";

export type TerminalOptions = {
  theme: TerminalTheme;
  fontFamily: string;
  fontSize: number;
  cursorStyle: "bar" | "block" | "underline";
  cursorBlink: boolean;
  scrollback: number;
};

type Phase = { kind: "starting" } | { kind: "live" } | { kind: "exited"; code: number | null } | { kind: "failed"; message: string };

/** One spawn per pane even when React mounts twice (StrictMode, reloads). */
const spawning = new Map<string, Promise<unknown>>();

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Attaches to a running PTY or spawns one. Each call gets its own channel:
 * when Rust drops a channel (a failed attach) the JS side closes it for good. */
async function connect(paneId: string, cols: number, rows: number, channel: () => Promise<FrameChannel>): Promise<{ replay?: Uint8Array; live: boolean }> {
  const pending = spawning.get(paneId);
  if (pending) await pending.catch(() => undefined);
  try {
    const snap = await invoke<AttachSnapshot>("pty_attach", { paneId, channel: await channel() });
    return { replay: base64ToBytes(snap.replay), live: snap.live };
  } catch (e) {
    if ((e as IpcError).code !== "not_found") throw e;
  }
  const spawn = invoke<PtyInfo>("pty_spawn", { paneId, cols, rows, channel: await channel() }, { timeoutMs: 15_000 });
  spawning.set(paneId, spawn);
  try {
    await spawn;
  } finally {
    spawning.delete(paneId);
  }
  return { live: true };
}

/** UI_SPEC §2.27 (M2 slice): xterm over a per-pane binary channel, with
 * ack-based flow control and the exited bar. */
export function TerminalView({ paneId, label, visible, options, onClose }: { paneId: string; label: string; visible: boolean; options: TerminalOptions; onClose?: () => void }) {
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const size = useRef({ cols: 0, rows: 0 });
  const [phase, setPhase] = useState<Phase>({ kind: "starting" });
  const [generation, setGeneration] = useState(0);
  const optionsRef = useRef(options);
  optionsRef.current = options;
  // An exited session still attaches (live: false); restart drops it first.
  const restart = () => {
    setPhase({ kind: "starting" });
    invoke("pty_kill", { paneId })
      .catch(() => undefined)
      .finally(() => setGeneration((g) => g + 1));
  };

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const o = optionsRef.current;
    const xterm = new Terminal({
      theme: o.theme,
      fontFamily: `"${o.fontFamily}", var(--font-mono), monospace`,
      fontSize: o.fontSize,
      lineHeight: 1.25,
      cursorStyle: o.cursorStyle === "underline" ? "underline" : o.cursorStyle,
      cursorBlink: o.cursorBlink,
      scrollback: o.scrollback,
      allowProposedApi: false,
    });
    const fitAddon = new FitAddon();
    xterm.loadAddon(fitAddon);
    xterm.open(el);
    term.current = xterm;
    fit.current = fitAddon;
    try {
      fitAddon.fit();
    } catch {
      /* hidden: fit later */
    }
    let disposed = false;
    let unacked = 0;
    let ackTimer: ReturnType<typeof setTimeout> | undefined;
    const flushAck = () => {
      ackTimer = undefined;
      if (unacked > 0) {
        const bytes = unacked;
        unacked = 0;
        invoke("pty_ack", { paneId, bytes }).catch(() => undefined);
      }
    };
    const onFrame = (f: Frame) => {
      if (disposed) return;
      if (f.kind === FrameKind.Data) {
        const len = f.payload.byteLength;
        xterm.write(f.payload, () => {
          unacked += len;
          if (unacked >= 64 * 1024) flushAck();
          else ackTimer ??= setTimeout(flushAck, 50);
        });
      } else if (f.kind === FrameKind.Exit) {
        const info = JSON.parse(new TextDecoder().decode(f.payload)) as ExitInfo;
        setPhase({ kind: "exited", code: info.code });
      } else if (f.kind === FrameKind.Notice) {
        const { message } = JSON.parse(new TextDecoder().decode(f.payload)) as { message: string };
        xterm.write(`\x1b[2m${message}\x1b[0m\r\n`);
      }
    };
    const input = xterm.onData((data) => invoke("pty_write", { paneId, data }).catch(() => undefined));
    void (async () => {
      try {
        const cols = Math.max(xterm.cols, 2);
        const rows = Math.max(xterm.rows, 2);
        size.current = { cols, rows };
        const { replay, live } = await connect(paneId, cols, rows, () => createFrameChannel(onFrame));
        if (disposed) return;
        if (replay?.byteLength) xterm.write(replay);
        setPhase(live ? { kind: "live" } : { kind: "exited", code: null });
      } catch (e) {
        if (disposed) return;
        const err = e as IpcError;
        log("warn", "pty.spawn_failed", { code: err.code ?? "internal" });
        setPhase({ kind: "failed", message: err.message ?? t("terminal.failed") });
      }
    })();
    return () => {
      disposed = true;
      clearTimeout(ackTimer);
      flushAck();
      input.dispose();
      xterm.dispose();
      term.current = null;
    };
  }, [paneId, generation]);

  // Options apply live: theme within one frame, no restart (F-SH-04 AC2).
  useEffect(() => {
    const xterm = term.current;
    if (!xterm) return;
    xterm.options.theme = options.theme;
    xterm.options.fontSize = options.fontSize;
    xterm.options.fontFamily = `"${options.fontFamily}", var(--font-mono), monospace`;
    xterm.options.cursorStyle = options.cursorStyle;
    xterm.options.cursorBlink = options.cursorBlink;
    xterm.options.scrollback = options.scrollback;
  }, [options]);

  // Resize once the layout settles (one PTY resize per change, §5.5).
  useEffect(() => {
    const el = host.current;
    if (!el || !visible) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const apply = () => {
      const f = fit.current;
      const xterm = term.current;
      if (!f || !xterm || el.clientWidth === 0) return;
      try {
        f.fit();
      } catch {
        return;
      }
      const next = { cols: xterm.cols, rows: xterm.rows };
      if (next.cols !== size.current.cols || next.rows !== size.current.rows) {
        size.current = next;
        invoke("pty_resize", { paneId, ...next }).catch(() => undefined);
      }
    };
    const ro = new ResizeObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(apply, 100);
    });
    ro.observe(el);
    apply();
    if (visible) term.current?.focus();
    return () => {
      ro.disconnect();
      clearTimeout(timer);
    };
  }, [paneId, visible]);

  return (
    <div role="group" aria-label={label} className="relative flex h-full min-h-0 flex-col overflow-hidden rounded-md border border-border" style={{ background: options.theme.background }}>
      <div ref={host} data-testid="terminal" data-pane-id={paneId} className="min-h-0 flex-1 px-2 py-1.5" />
      {phase.kind === "starting" && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center gap-2 text-sm text-fg-muted">
          <Spinner />
          {t("terminal.starting")}
        </div>
      )}
      {phase.kind === "failed" && (
        <div role="alert" className="absolute inset-x-0 bottom-0 flex items-center gap-3 border-t border-border bg-surface-3 px-3 py-2 text-sm">
          <span className="flex-1 text-fg">{phase.message}</span>
          <Button size="sm" onClick={restart}>
            {t("terminal.restart")}
          </Button>
        </div>
      )}
      {phase.kind === "exited" && (
        <div role="status" className="absolute inset-x-0 bottom-0 flex items-center gap-3 border-t border-border bg-surface-3 px-3 py-2 text-sm">
          <span className="flex-1 text-fg">{t("terminal.exited", { code: phase.code ?? "?" })}</span>
          <Button size="sm" variant="primary" onClick={restart}>
            {t("terminal.restart")}
          </Button>
          {onClose && (
            <Button size="sm" variant="ghost" onClick={onClose}>
              {t("terminal.close")}
            </Button>
          )}
        </div>
      )}
    </div>
  );
}
