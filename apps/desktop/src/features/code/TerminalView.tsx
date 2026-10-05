import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal, type IMarker } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { ArrowDown } from "lucide-react";
import type { ExitInfo, IpcError, LastOutput, PtyInfo, AttachSnapshot, PtyState } from "@tandem/protocol";
import { Button, ConfirmDialog, Spinner, cx, type TerminalTheme } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { log } from "../../ipc/log";
import { createFrameChannel, FrameKind, type Frame, type FrameChannel } from "../../ipc/pty";
import { useLayout } from "../../stores/layout";
import { useTerminals } from "../../stores/terminals";
import { copyText, readText } from "../../ipc/clipboard";
import { CommandMarks, type Activity } from "./commandMarks";
import { HIGHLIGHT_LIMIT, searchOptions, validQuery, type FindFlags, type FindResult } from "./find";
import { FindBar } from "./FindBar";
import { keyAction, lastLines, lineCount, needsConfirm } from "./terminalInput";
import { webglPool } from "./webglPool";

export type TerminalOptions = {
  theme: TerminalTheme;
  fontFamily: string;
  fontSize: number;
  cursorStyle: "bar" | "block" | "underline";
  cursorBlink: boolean;
  scrollback: number;
  /** Settings › Terminal: GPU rendering (off keeps every pane on the DOM renderer). */
  gpu?: boolean;
  /** "Paste N lines?" before multi-line pastes into shells (default on). */
  confirmMultilinePaste?: boolean;
  copyOnSelect?: boolean;
  optionAsMeta?: boolean;
  screenReader?: boolean;
};

/** The bundled code font (self-hosted, so it never comes from the network). */
const BUNDLED_MONO = "JetBrains Mono Variable";

/** The chosen font, the bundled JetBrains Mono, then CJK and emoji fallbacks
 * (F-CD-04). Plain family names only: the WebGL renderer measures with a
 * canvas, which can't resolve CSS variables. */
export function fontStack(family: string): string {
  const names = [family, BUNDLED_MONO, "JetBrains Mono", "ui-monospace", "SF Mono", "Menlo", "Consolas", "Noto Sans Mono CJK SC", "Noto Sans CJK SC", "Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji"];
  return [...new Set(names)].map((n) => (n === "ui-monospace" ? n : `"${n}"`)).join(", ") + ", monospace";
}

/** Unfocused panes coalesce output to at most ~30 fps (F-CD-04). */
const UNFOCUSED_FLUSH_MS = 33;

export type Phase =
  | { kind: "starting" }
  | { kind: "live" }
  | { kind: "exited"; code: number | null }
  | { kind: "failed"; message: string };

/** Actions other parts of the UI run on a pane's terminal (menus, shortcuts). */
export type TerminalHandle = {
  restart: () => void;
  focus: () => void;
  jumpToBottom: () => void;
  /** Copies the last `n` lines (all with `null`), ANSI-free (F-CD-05 AC4). */
  copyLast: (n: number | null) => Promise<boolean>;
  /** The buffer as text, whatever renders it (tests read this: WebGL draws to a canvas). */
  text: () => string;
  /** Opens the find bar, or steps through matches once it has a query (F-CD-08). */
  find: (step?: "next" | "prev") => void;
  /** Copies the last finished command's output (shell integration, F-CD-10 AC3). */
  copyLastOutput: () => Promise<"copied" | "none">;
};
export const terminalHandles = new Map<string, TerminalHandle>();

type Toast = (t: { tone?: "info" | "success" | "warning" | "error"; title: string }) => void;

/** "Copy last command output" from the pane menu and the palette (F-CD-10 AC3). */
export async function copyLastOutput(paneId: string, toast: Toast): Promise<void> {
  const result = (await terminalHandles.get(paneId)?.copyLastOutput()) ?? "none";
  toast(result === "copied" ? { tone: "success", title: t("terminal.copied") } : { tone: "info", title: t("terminal.noLastOutput") });
}

/** Terminals reflow at most this often while their size changes (F-CD-02). */
const RESIZE_THROTTLE_MS = 50;

/** One spawn per pane even when React mounts twice (StrictMode, reloads). */
const spawning = new Map<string, Promise<unknown>>();

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Rust answers terminal queries for every pane, visible or hidden, so each
 * is answered exactly once (ARCHITECTURE §5.7, F-CD-03 AC5). xterm's own
 * replies to the same queries are switched off. */
export function quietQueries(xterm: Pick<Terminal, "parser">): void {
  xterm.parser.registerCsiHandler({ final: "c" }, () => true); // DA1
  xterm.parser.registerCsiHandler({ prefix: ">", final: "c" }, () => true); // DA2
  xterm.parser.registerCsiHandler({ final: "n" }, (params) => params[0] === 5 || params[0] === 6); // DSR, CPR
  xterm.parser.registerCsiHandler({ prefix: ">", final: "q" }, () => true); // XTVERSION
  for (const code of [10, 11, 12]) xterm.parser.registerOscHandler(code, (data) => data === "?"); // colour queries
}

/** Attaches to a running PTY or spawns one. Each call gets its own channel:
 * when Rust drops a channel (a failed attach) the JS side closes it for good. */
async function connect(
  paneId: string,
  cols: number,
  rows: number,
  channel: () => Promise<FrameChannel>,
): Promise<{ replay?: Uint8Array; live: boolean; state?: PtyState | null }> {
  const pending = spawning.get(paneId);
  if (pending) await pending.catch(() => undefined);
  try {
    const snap = await invoke<AttachSnapshot>("pty_attach", { paneId, channel: await channel() });
    return { replay: base64ToBytes(snap.replay), live: snap.live, state: snap.state };
  } catch (e) {
    if ((e as IpcError).code !== "not_found") throw e;
  }
  const spawn = invoke<PtyInfo>(
    "pty_spawn",
    { paneId, cols, rows, channel: await channel() },
    { timeoutMs: 15_000 },
  );
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
export function TerminalView({
  paneId,
  label,
  visible,
  options,
  onClose,
  focused = true,
  bare = false,
  onPhase,
  agent = false,
  newlineSequence = "\x1b\r",
  mac = false,
  onActivity,
}: {
  paneId: string;
  label: string;
  visible: boolean;
  options: TerminalOptions;
  onClose?: () => void;
  /** The tab's focused pane: becoming focused moves keyboard focus here. */
  focused?: boolean;
  /** Inside a pane frame (no border of its own). */
  bare?: boolean;
  onPhase?: (phase: Phase) => void;
  /** An agent pane: Shift+Enter types `newlineSequence`; images paste as staged files. */
  agent?: boolean;
  newlineSequence?: string;
  mac?: boolean;
  /** Shell integration: a command is running, or the shell went quiet (F-CD-10). */
  onActivity?: (activity: Activity) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const size = useRef({ cols: 0, rows: 0 });
  const [phase, setPhase] = useState<Phase>({ kind: "starting" });
  const [generation, setGeneration] = useState(0);
  const [scrolledUp, setScrolledUp] = useState(false);
  const [pendingPaste, setPendingPaste] = useState<string | null>(null);
  const search = useRef<SearchAddon | null>(null);
  const findInput = useRef<HTMLInputElement>(null);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [findFlags, setFindFlags] = useState<FindFlags>({ caseSensitive: false, regex: false });
  const [findResult, setFindResult] = useState<FindResult | null>(null);
  const inputCtx = useRef({ agent, newlineSequence, mac });
  inputCtx.current = { agent, newlineSequence, mac };
  const flushNow = useRef<() => void>(() => undefined);
  const optionsRef = useRef(options);
  optionsRef.current = options;
  const visibleRef = useRef(visible);
  visibleRef.current = visible;
  const focusedRef = useRef(focused);
  focusedRef.current = focused;
  const dragging = useLayout((st) => st.dragging);
  const onPhaseRef = useRef(onPhase);
  onPhaseRef.current = onPhase;
  const onActivityRef = useRef(onActivity);
  onActivityRef.current = onActivity;
  useEffect(() => onPhaseRef.current?.(phase), [phase]);
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
      fontFamily: fontStack(o.fontFamily),
      fontSize: o.fontSize,
      lineHeight: 1.25,
      cursorStyle: o.cursorStyle === "underline" ? "underline" : o.cursorStyle,
      cursorBlink: o.cursorBlink,
      scrollback: o.scrollback,
      // The Unicode 11 width tables need the proposed API.
      allowProposedApi: true,
      macOptionIsMeta: o.optionAsMeta ?? false,
      screenReaderMode: o.screenReader ?? false,
      scrollOnUserInput: true,
      smoothScrollDuration: 0,
      fontWeightBold: "bold",
    });
    const fitAddon = new FitAddon();
    xterm.loadAddon(fitAddon);
    xterm.loadAddon(new Unicode11Addon());
    xterm.unicode.activeVersion = "11";
    const searchAddon = new SearchAddon({ highlightLimit: HIGHLIGHT_LIMIT });
    xterm.loadAddon(searchAddon);
    search.current = searchAddon;
    const results = searchAddon.onDidChangeResults((r) => setFindResult({ index: r.resultIndex, count: r.resultCount }));
    xterm.open(el);
    quietQueries(xterm);
    // Shell-integration marks reach xterm only when they carried this
    // launch's token (Rust drops the rest): ✓/✗ beside each command's prompt
    // and working/idle for the pane's status (F-CD-10).
    const commands = new CommandMarks<IMarker>({
      marker: () => xterm.registerMarker(0),
      release: (m) => m.dispose(),
      decorate: (marker, ok) => {
        const deco = xterm.registerDecoration({ marker, x: 0, width: 1, layer: "top" });
        deco?.onRender((node) => {
          node.classList.add("tn-cmd-mark");
          node.dataset.ok = String(ok);
          node.textContent = ok ? "✓" : "✗";
          node.setAttribute("aria-hidden", "true");
        });
      },
      activity: (a) => onActivityRef.current?.(a),
      now: () => performance.now(),
      setTimer: (fn, ms) => setTimeout(fn, ms),
      clearTimer: (t) => clearTimeout(t as ReturnType<typeof setTimeout>),
    });
    const commandMarks = xterm.parser.registerOscHandler(133, (data) => {
      commands.mark(data);
      return true;
    });
    // A WebGL renderer when the pool grants one; the DOM renderer otherwise.
    let webgl: WebglAddon | undefined;
    const renderer = {
      attach: () => {
        if (optionsRef.current.gpu === false) return false;
        try {
          const addon = new WebglAddon();
          addon.onContextLoss(() => webglPool.lost(paneId));
          xterm.loadAddon(addon);
          webgl = addon;
          return true;
        } catch {
          return false;
        }
      },
      detach: () => {
        webgl?.dispose();
        webgl = undefined;
      },
    };
    webglPool.register(paneId, renderer);
    // Cell size is measured from the font; once the web font has loaded,
    // measure again so canvas glyphs line up with the grid.
    const fonts = (document as Document & { fonts?: FontFaceSet }).fonts;
    if (fonts && !fonts.check(`${o.fontSize}px "${BUNDLED_MONO}"`)) {
      void fonts.load(`${o.fontSize}px "${BUNDLED_MONO}"`).then(() => {
        if (term.current !== xterm) return;
        const family = xterm.options.fontFamily ?? fontStack(o.fontFamily);
        xterm.options.fontFamily = "monospace";
        xterm.options.fontFamily = family;
        try {
          fitAddon.fit();
        } catch {
          /* hidden */
        }
      });
    }
    const scrolled = () => {
      const buf = xterm.buffer.active;
      setScrolledUp(buf.viewportY < buf.baseY);
    };
    const onScroll = xterm.onScroll(scrolled);
    const onParsed = xterm.onWriteParsed(scrolled);
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
    const write = (data: Uint8Array) => {
      const len = data.byteLength;
      xterm.write(data, () => {
        unacked += len;
        if (unacked >= 64 * 1024) flushAck();
        else ackTimer ??= setTimeout(flushAck, 50);
      });
    };
    // The focused pane writes at once (xterm renders at up to 60 fps); the
    // others coalesce into one write per ~33 ms.
    let pending: Uint8Array[] = [];
    let flushTimer: ReturnType<typeof setTimeout> | undefined;
    const flushPending = () => {
      flushTimer = undefined;
      if (pending.length === 0) return;
      const total = pending.reduce((n, c) => n + c.byteLength, 0);
      const merged = new Uint8Array(total);
      let at = 0;
      for (const c of pending) {
        merged.set(c, at);
        at += c.byteLength;
      }
      pending = [];
      write(merged);
    };
    flushNow.current = flushPending;
    const onFrame = (f: Frame) => {
      if (disposed) return;
      if (f.kind === FrameKind.Data) {
        commands.output();
        if (focusedRef.current && pending.length === 0) {
          write(f.payload);
        } else {
          pending.push(f.payload.slice());
          flushTimer ??= setTimeout(flushPending, UNFOCUSED_FLUSH_MS);
        }
      } else if (f.kind === FrameKind.Exit) {
        // The final output always renders before the exit notice.
        flushPending();
        const info = JSON.parse(new TextDecoder().decode(f.payload)) as ExitInfo;
        setPhase({ kind: "exited", code: info.code });
      } else if (f.kind === FrameKind.Notice) {
        const { message } = JSON.parse(new TextDecoder().decode(f.payload)) as { message: string };
        xterm.write(`\x1b[2m${message}\x1b[0m\r\n`);
      }
    };
    const input = xterm.onData((data) => invoke("pty_write", { paneId, data }).catch(() => undefined));
    // Copy, paste and the agent newline (F-CD-05, F-CD-07); everything else
    // reaches the PTY.
    xterm.attachCustomKeyEventHandler((ev) => {
      const action = keyAction(ev, { ...inputCtx.current, hasSelection: xterm.hasSelection() });
      if (action === "pass") return true;
      if (ev.type !== "keydown") return false;
      ev.preventDefault();
      if (action === "copy") {
        const text = xterm.getSelection();
        if (text) void copyText(text);
      } else if (action === "paste") {
        void pasteRef.current(undefined);
      } else if (action === "newline") {
        invoke("pty_write", { paneId, data: inputCtx.current.newlineSequence }).catch(() => undefined);
      }
      return false;
    });
    // Native paste events (menu, middle click) take the same path.
    const onNativePaste = (e: ClipboardEvent) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      void pasteRef.current(e.clipboardData?.getData("text/plain") ?? undefined);
    };
    xterm.textarea?.addEventListener("paste", onNativePaste, true);
    const selection = xterm.onSelectionChange(() => {
      if (optionsRef.current.copyOnSelect && xterm.hasSelection()) void copyText(xterm.getSelection());
    });
    void (async () => {
      try {
        const cols = Math.max(xterm.cols, 2);
        const rows = Math.max(xterm.rows, 2);
        size.current = { cols, rows };
        const { replay, live, state } = await connect(paneId, cols, rows, () => createFrameChannel(onFrame));
        if (state) useTerminals.getState().put(state);
        if (disposed) return;
        if (replay?.byteLength) xterm.write(replay);
        setPhase(live ? { kind: "live" } : { kind: "exited", code: null });
        // Focus a new shell once; later visibility changes leave focus alone
        // so closing Settings can return it to its opener (F-SH-04 AC1).
        if (focusedRef.current && el.offsetParent !== null && !document.querySelector("[role=dialog]"))
          xterm.focus();
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
      clearTimeout(flushTimer);
      flushAck();
      input.dispose();
      selection.dispose();
      xterm.textarea?.removeEventListener("paste", onNativePaste, true);
      onScroll.dispose();
      onParsed.dispose();
      results.dispose();
      search.current = null;
      commandMarks.dispose();
      commands.dispose();
      webglPool.unregister(paneId, renderer);
      flushNow.current = () => undefined;
      xterm.dispose();
      term.current = null;
    };
  }, [paneId, generation]);

  // Visibility and focus decide who holds a WebGL context (§5.6).
  useEffect(() => {
    webglPool.update(paneId, { visible, focused });
    // Becoming focused flushes coalesced output at once.
    if (focused) flushNow.current();
  }, [paneId, visible, focused, generation]);

  const jumpToBottom = () => {
    term.current?.scrollToBottom();
    setScrolledUp(false);
    term.current?.focus();
  };

  /** Pastes `given` text, or the clipboard: confirms multi-line shell pastes,
   * stages an image for agents, and lets xterm add bracketed paste markers. */
  const pasteFrom = async (given: string | undefined) => {
    const xterm = term.current;
    if (!xterm) return;
    const text = given ?? (await readText());
    if (!text) {
      if (!inputCtx.current.agent) return;
      const staged = await invoke<string>("term_stage_clipboard_image", { paneId }).catch(() => "");
      if (staged) xterm.paste(staged);
      return;
    }
    if (needsConfirm(text, { agent: inputCtx.current.agent, confirmSetting: optionsRef.current.confirmMultilinePaste ?? true })) {
      setPendingPaste(text);
      return;
    }
    xterm.paste(text);
  };

  /** Runs the search addon: `incremental` while typing, so the current match
   * grows rather than moving on. */
  const runFind = (dir: "next" | "prev", query: string, flags: FindFlags, incremental = false) => {
    const s = search.current;
    if (!s) return;
    if (!query || !validQuery(query, flags)) {
      s.clearDecorations();
      term.current?.clearSelection();
      setFindResult(null);
      return;
    }
    const opts = searchOptions(flags, optionsRef.current.theme, incremental);
    if (dir === "prev") s.findPrevious(query, opts);
    else s.findNext(query, opts);
  };

  const openFind = () => {
    setFindOpen(true);
    // Focus after the bar renders; select so typing replaces the last query.
    requestAnimationFrame(() => {
      findInput.current?.focus();
      findInput.current?.select();
    });
  };

  const closeFind = () => {
    setFindOpen(false);
    search.current?.clearDecorations();
    setFindResult(null);
    term.current?.focus();
  };

  const findStep = (step?: "next" | "prev") => {
    if (!step || !findOpen || !findQuery) openFind();
    else runFind(step, findQuery, findFlags);
  };

  const pasteRef = useRef(pasteFrom);
  pasteRef.current = pasteFrom;

  const copyLast = async (n: number | null) => {
    const xterm = term.current;
    if (!xterm) return false;
    const buf = xterm.buffer.active;
    const lines: string[] = [];
    for (let i = 0; i < buf.length; i++) lines.push(buf.getLine(i)?.translateToString(true) ?? "");
    return copyText(lastLines(lines, n));
  };

  useEffect(() => {
    const text = () => {
      const buf = term.current?.buffer.active;
      if (!buf) return "";
      const lines: string[] = [];
      for (let i = 0; i < buf.length; i++) lines.push(buf.getLine(i)?.translateToString(true) ?? "");
      return lastLines(lines, null);
    };
    const copyLastOutput = async () => {
      const last = await invoke<LastOutput | null>("term_last_output", { paneId }).catch(() => null);
      if (!last) return "none" as const;
      return (await copyText(last.text)) ? ("copied" as const) : ("none" as const);
    };
    const handle: TerminalHandle = {
      restart,
      focus: () => term.current?.focus(),
      jumpToBottom,
      copyLast,
      text,
      find: findStep,
      copyLastOutput,
    };
    terminalHandles.set(paneId, handle);
    return () => {
      // Another view of the same pane may have taken over (a remount).
      if (terminalHandles.get(paneId) === handle) terminalHandles.delete(paneId);
    };
  });

  // A pane that becomes the focused one (click, Mod+Alt+Arrow, a new split)
  // takes keyboard focus, unless a dialog is open.
  const focusChanged = useRef(false);
  useEffect(() => {
    // A new pane takes focus once, when its shell connects (below); this
    // only follows later changes.
    if (!focusChanged.current) {
      focusChanged.current = true;
      return;
    }
    if (!focused || !visibleRef.current || document.querySelector("[role=dialog]")) return;
    const el = host.current;
    if (el && !el.contains(document.activeElement)) term.current?.focus();
  }, [focused]);

  // Options apply live: theme within one frame, no restart (F-SH-04 AC2).
  useEffect(() => {
    const xterm = term.current;
    if (!xterm) return;
    xterm.options.theme = options.theme;
    xterm.options.fontSize = options.fontSize;
    xterm.options.fontFamily = fontStack(options.fontFamily);
    xterm.options.macOptionIsMeta = options.optionAsMeta ?? false;
    xterm.options.screenReaderMode = options.screenReader ?? false;
    xterm.options.cursorStyle = options.cursorStyle;
    xterm.options.cursorBlink = options.cursorBlink;
    xterm.options.scrollback = options.scrollback;
  }, [options]);

  // PTY resizes follow the layout: never during a divider drag (one resize
  // after pointer-up, F-CD-02 AC1), at most every 50 ms otherwise.
  useEffect(() => {
    const el = host.current;
    if (!el || !visible || dragging) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let last = 0;
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
      if (timer) return;
      timer = setTimeout(
        () => {
          timer = undefined;
          last = performance.now();
          apply();
        },
        Math.max(0, RESIZE_THROTTLE_MS - (performance.now() - last)),
      );
    });
    ro.observe(el);
    last = performance.now();
    apply();
    return () => {
      ro.disconnect();
      clearTimeout(timer);
    };
  }, [paneId, visible, dragging]);

  return (
    <div
      role="group"
      aria-label={label}
      className={cx(
        "relative flex h-full min-h-0 flex-col overflow-hidden",
        !bare && "rounded-md border border-border",
      )}
      style={{ background: options.theme.background }}
    >
      <div ref={host} data-testid="terminal" data-pane-id={paneId} className="tn-term min-h-0 flex-1 py-1.5 pl-3.5 pr-2" />
      {scrolledUp && phase.kind !== "starting" && (
        <button
          type="button"
          data-testid="jump-bottom"
          onClick={jumpToBottom}
          className="absolute bottom-3 right-4 z-10 inline-flex h-7 items-center gap-1 rounded-full border border-border-strong bg-surface-3 px-3 text-sm text-fg shadow-2 hover:bg-surface-4 focus-visible:shadow-focus focus-visible:outline-none"
        >
          {t("terminal.jumpBottom")}
          <ArrowDown size={13} aria-hidden="true" />
        </button>
      )}
      {findOpen && (
        <FindBar
          ref={findInput}
          query={findQuery}
          flags={findFlags}
          result={findResult}
          valid={validQuery(findQuery, findFlags)}
          mac={mac}
          onQuery={(q) => {
            setFindQuery(q);
            runFind("next", q, findFlags, true);
          }}
          onFlags={(f) => {
            setFindFlags(f);
            runFind("next", findQuery, f, true);
          }}
          onStep={(dir) => runFind(dir, findQuery, findFlags)}
          onClose={closeFind}
        />
      )}
      {phase.kind === "starting" && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center gap-2 text-sm text-fg-muted">
          <Spinner />
          {t("terminal.starting")}
        </div>
      )}
      {phase.kind === "failed" && (
        <div
          role="alert"
          className="absolute inset-x-0 bottom-0 flex items-center gap-3 border-t border-border bg-surface-3 px-3 py-2 text-sm"
        >
          <span className="flex-1 text-fg">{phase.message}</span>
          <Button size="sm" onClick={restart}>
            {t("terminal.restart")}
          </Button>
        </div>
      )}
      {phase.kind === "exited" && (
        <div
          role="status"
          className="absolute inset-x-0 bottom-0 flex items-center gap-3 border-t border-border bg-surface-3 px-3 py-2 text-sm"
        >
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
      <ConfirmDialog
        open={pendingPaste !== null}
        onOpenChange={(o) => !o && setPendingPaste(null)}
        title={t("terminal.pasteLines", { n: lineCount(pendingPaste ?? "") })}
        body={t("terminal.pasteBody")}
        confirmLabel={t("terminal.paste")}
        onConfirm={() => {
          if (pendingPaste !== null) term.current?.paste(pendingPaste);
          setPendingPaste(null);
          term.current?.focus();
        }}
      />
    </div>
  );
}
