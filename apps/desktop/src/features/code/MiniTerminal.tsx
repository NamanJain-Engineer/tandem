import { useEffect, useRef } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import type { AttachSnapshot } from "@tandem/protocol";
import { terminalThemeFor, useTheme } from "@tandem/ui";
import { invoke } from "../../ipc/invoke";
import { createFrameChannel, FrameKind } from "../../ipc/pty";
import { useSetting } from "../../stores/settings";
import { base64ToBytes, fontStack, remeasure } from "./TerminalView";

/** A small terminal on a PTY that isn't a pane: the engine's own sign-in
 * inside Settings › Accounts (F-AC-02, UI-35). It fills its box, and the PTY
 * follows its size. */
export function MiniTerminal({
  paneId,
  label,
  cols = 100,
  rows = 20,
}: {
  paneId: string;
  label: string;
  cols?: number;
  rows?: number;
}) {
  const host = useRef<HTMLDivElement>(null);
  const { resolved, highContrast } = useTheme();
  const fontFamily = useSetting("term_font_family", "JetBrains Mono");
  const fontSize = useSetting("term_font_size", 13);
  const theme = terminalThemeFor("follow_app", resolved, highContrast);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const term = new Terminal({
      cols,
      rows,
      fontFamily: fontStack(fontFamily),
      fontSize,
      theme,
      cursorBlink: true,
      scrollback: 2_000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    let gone = false;
    const refit = () => {
      if (gone) return;
      fit.fit();
      void invoke("pty_resize", { paneId, cols: term.cols, rows: term.rows }).catch(() => undefined);
    };
    refit();
    // Measured again once the font has loaded: a fallback's metrics space
    // the letters out.
    void document.fonts?.ready.then(() => {
      if (gone) return;
      remeasure(term);
      refit();
    });
    const typed = term.onData((data) => {
      void invoke("pty_write", { paneId, data }).catch(() => undefined);
    });
    void createFrameChannel((f) => {
      if (gone) return;
      if (f.kind !== FrameKind.Data) return;
      // Acked once parsed, as panes do: the core holds output back otherwise.
      const bytes = f.payload.byteLength;
      term.write(f.payload, () => void invoke("pty_ack", { paneId, bytes }).catch(() => undefined));
    }).then(async (channel) => {
      const snap = await invoke<AttachSnapshot>("pty_attach", { paneId, channel }).catch(() => null);
      if (snap && !gone) term.write(base64ToBytes(snap.replay));
    });
    term.focus();
    return () => {
      gone = true;
      typed.dispose();
      term.dispose();
    };
    // The PTY is the identity; theme and font changes don't restart it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paneId]);

  return (
    <div
      ref={host}
      role="group"
      aria-label={label}
      data-testid="mini-terminal"
      data-pane-id={paneId}
      className="h-[320px] overflow-hidden rounded-md border border-border bg-surface-1 p-2"
    />
  );
}
