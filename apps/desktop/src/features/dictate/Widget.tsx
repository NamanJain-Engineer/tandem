import { useEffect, useRef, useState } from "react";
import { Check, Copy, Lock, TriangleAlert, X } from "lucide-react";
import type { DictateStateEvent, DictateStatus, DictateToast, RewriteMode } from "@tandem/protocol";
import { Button, WaveformRing, cx, type WaveformState } from "@tandem/ui";
import { t } from "../../i18n";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { ToastActions } from "./ToastActions";
import { toastText } from "./toasts";

/** mm:ss */
export function clock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

const WARN_MS = 570_000;

type View =
  | { kind: "idle" }
  | { kind: "listening"; since: number; handsFree: boolean }
  | { kind: "transcribing" }
  | { kind: "inserted"; words: number }
  | { kind: "copied" }
  | { kind: "error"; toast: DictateToast | null }
  | { kind: "reconnecting" }
  | { kind: "rewriting"; mode: RewriteMode };

/** What the pill shows for a state event; toasts can replace it. */
export function viewFor(e: DictateStateEvent, now: number): View {
  switch (e.state) {
    case "listening":
      return { kind: "listening", since: now, handsFree: e.handsFree };
    case "transcribing":
      return { kind: "transcribing" };
    case "inserted":
      return { kind: "inserted", words: e.words ?? 0 };
    case "copied":
      return { kind: "copied" };
    case "error":
      return { kind: "error", toast: null };
    case "reconnecting":
      return { kind: "reconnecting" };
    case "enhancing":
      return { kind: "rewriting", mode: "enhance" };
    case "polishing":
      return { kind: "rewriting", mode: "polish" };
    default:
      return { kind: "idle" };
  }
}

/** The pill for a take already running when the window opens: it's made
 * on the first recording (D-519), after that take's first event. */
export function viewFromStatus(state: DictateStatus["state"], now: number): View | null {
  switch (state) {
    case "arming":
    case "recording":
      return { kind: "listening", since: now, handsFree: false };
    case "transcribing":
    case "post_processing":
    case "inserting":
      return { kind: "transcribing" };
    default:
      return null;
  }
}

/** UI-60: the Dictate widget, alone in its transparent window. */
export function DictateWidget() {
  const [status, setStatus] = useState<DictateStatus | null>(null);
  const [view, setView] = useState<View>({ kind: "idle" });
  const [level, setLevel] = useState(0);
  const [now, setNow] = useState(() => Date.now());
  const [toast, setToast] = useState<DictateToast | null>(null);
  const [disclose, setDisclose] = useState<RewriteMode | null>(null);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    void invoke<DictateStatus>("dictate_status").then((s) => {
      setStatus(s);
      // Only when no event has arrived yet: an event is newer than this.
      const seed = viewFromStatus(s.state, Date.now());
      if (seed) setView((v) => (v.kind === "idle" ? seed : v));
    }, () => undefined);
    const offs = [
      listen<DictateStatus>("dictate:status", setStatus),
      listen<DictateStateEvent>("dictate:state", (e) => {
        if (e.composer) return;
        setView(viewFor(e, Date.now()));
        if (e.state === "listening") setToast(null);
      }),
      listen<number>("dictate:level", setLevel),
      listen<DictateToast>("dictate:toast", (tst) => {
        setToast(tst);
        if (toastTimer.current) clearTimeout(toastTimer.current);
        toastTimer.current = setTimeout(() => setToast(null), tst.error ? 6_000 : 4_000);
      }),
    ];
    return () => {
      offs.forEach((p) => void p.then((off) => off()));
      if (toastTimer.current) clearTimeout(toastTimer.current);
    };
  }, []);

  useEffect(() => {
    if (view.kind !== "listening") return;
    const id = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(id);
  }, [view.kind]);

  const style = status?.widgetStyle ?? "logo_text";
  const showText = style !== "logo";
  const mono = style === "mono";
  const key = status?.hold.label ?? "";
  const cancel = () => void invoke("dictate_cancel_take").catch(() => undefined);
  // Enhance and Polish after a take (F-DI-10); the first click shows the
  // one-time disclosure in the pill instead.
  const quick = (mode: RewriteMode) => {
    if (!status?.enhanceDisclosed) return setDisclose(mode);
    void invoke<boolean>("dictate_quick_action", { mode }).catch(() => false);
  };

  let ring: WaveformState = "idle";
  let text = t("dictate.widget.idle");
  let icon: React.ReactNode = null;
  let tail: React.ReactNode = null;
  let error = false;
  if (disclose) {
    text = t("dictate.enhance.disclosureShort");
    tail = (
      <span className="flex shrink-0 items-center gap-1">
        <Button size="sm" variant="ghost" onClick={() => setDisclose(null)}>
          {t("dictionary.cancel")}
        </Button>
        <Button
          size="sm"
          variant="secondary"
          data-testid="widget-disclosure-continue"
          onClick={() => {
            const mode = disclose;
            setDisclose(null);
            void invoke<boolean>("dictate_quick_action", { mode, disclosed: true }).catch(() => false);
          }}
        >
          {t("dictate.enhance.continue")}
        </Button>
      </span>
    );
  } else if (toast && (toast.error || view.kind !== "listening")) {
    error = toast.error;
    ring = toast.error ? "error" : "idle";
    text = toastText(toast);
    if (toast.actions.length > 0) tail = <ToastActions actions={toast.actions} onDone={() => setToast(null)} />;
    if (toast.code === "copied" || toast.code === "elevated") icon = <Copy size={16} aria-hidden="true" />;
    else if (toast.error) icon = <TriangleAlert size={16} aria-hidden="true" className="text-[var(--tn-status-error)]" />;
  } else if (view.kind === "listening") {
    ring = "listening";
    const elapsed = now - view.since;
    text = view.handsFree ? t("dictate.widget.handsFree", { key }) : t("dictate.widget.listening");
    tail = (
      <>
        <span
          className={cx("font-mono text-sm tabular-nums", elapsed >= WARN_MS ? "text-[var(--tn-status-waiting-text)]" : "text-fg-muted")}
          data-testid="widget-timer"
        >
          {elapsed >= WARN_MS ? t("dictate.widget.left", { s: Math.max(0, Math.ceil((600_000 - elapsed) / 1000)) }) : clock(elapsed)}
        </span>
        <button
          type="button"
          aria-label={t("dictate.widget.cancel")}
          onClick={cancel}
          className="inline-flex h-6 w-6 items-center justify-center rounded-full text-fg-muted hover:bg-surface-4 hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
        >
          <X size={14} aria-hidden="true" />
        </button>
      </>
    );
    if (view.handsFree) icon = <Lock size={12} aria-hidden="true" />;
  } else if (view.kind === "transcribing" || view.kind === "reconnecting") {
    ring = "thinking";
    text = view.kind === "transcribing" ? t("dictate.widget.transcribing") : t("dictate.widget.reconnecting");
  } else if (view.kind === "rewriting") {
    ring = "thinking";
    text = view.mode === "enhance" ? t("dictate.widget.enhancing") : t("dictate.widget.polishing");
    tail = <span className="text-sm text-fg-muted">{t("dictate.enhance.cost")}</span>;
  } else if (view.kind === "inserted") {
    icon = <Check size={16} aria-hidden="true" className="text-[var(--tn-status-done)]" />;
    text = t("dictate.widget.inserted", { n: view.words });
    if (status?.quickActions)
      tail = (
        <span className="flex shrink-0 items-center gap-1" data-testid="widget-quick-actions">
          <Button size="sm" variant="ghost" onClick={() => quick("enhance")} data-testid="widget-enhance">
            {t("dictate.enhance.enhanceAction")}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => quick("polish")} data-testid="widget-polish">
            {t("dictate.enhance.polishAction")}
          </Button>
        </span>
      );
  } else if (view.kind === "copied") {
    icon = <Copy size={16} aria-hidden="true" />;
    text = t("dictate.toast.copied", { mod: navigator.platform.includes("Mac") ? "⌘" : "Ctrl+" });
  }
  const showRing = !icon || view.kind === "listening";
  const pill = (
    <div
      data-tauri-drag-region
      data-testid="dictate-widget"
      data-state={toast ? (toast.error ? "error" : "toast") : view.kind}
      role="status"
      // The full diagnostic on hover (F-DI-07 AC2): a native tooltip, which
      // the OS draws outside this small window.
      title={toast?.detail}
      aria-description={toast?.detail}
      className={cx(
        "flex h-10 items-center gap-2 rounded-full border border-border bg-surface-3 pl-1.5 pr-2 shadow-3 select-none",
        disclose || (toast && toast.actions.length > 0) || (view.kind === "inserted" && status?.quickActions)
          ? "w-auto max-w-[540px]"
          : error ? "w-[360px]" : showText ? "w-[280px]" : "w-auto",
        mono && "grayscale",
      )}
      onMouseUp={() => {
        void import("@tauri-apps/api/window")
          .then(({ getCurrentWindow }) => getCurrentWindow().outerPosition())
          .then((p) => invoke("dictate_widget_set_position", { x: p.x, y: p.y, locked: status?.lockPosition ?? false }))
          .catch(() => undefined);
      }}
    >
      {showRing && <WaveformRing size={28} state={ring} level={level} label={text} />}
      {icon && <span className="inline-flex w-5 justify-center">{icon}</span>}
      {showText && (
        <span className="min-w-0 flex-1 truncate text-sm text-fg" data-testid="widget-text">
          {text}
        </span>
      )}
      {tail}
    </div>
  );
  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent">
      {pill}
    </div>
  );
}
