import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import { cx } from "../cx";
import { Button } from "./Button";

export type ToastTone = "info" | "success" | "warning" | "error";
export type ToastInput = {
  tone?: ToastTone;
  title: string;
  body?: string;
  action?: { label: string; onClick: () => void };
  /** ms; errors persist until dismissed. */
  duration?: number;
};
type ToastItem = ToastInput & { id: number };

type Ctx = { show: (t: ToastInput) => number; dismiss: (id: number) => void };
const ToastContext = createContext<Ctx | null>(null);

const BAR: Record<ToastTone, string> = {
  info: "bg-info",
  success: "bg-success",
  warning: "bg-warning",
  error: "bg-danger",
};

function ToastCard({ item, onDismiss, dismissLabel }: { item: ToastItem; onDismiss: () => void; dismissLabel: string }) {
  const tone = item.tone ?? "info";
  const persistent = tone === "error" && item.duration === undefined;
  const [paused, setPaused] = useState(false);
  const remaining = useRef(item.duration ?? 5000);
  const started = useRef(Date.now());
  useEffect(() => {
    if (persistent || paused) return;
    started.current = Date.now();
    const t = setTimeout(onDismiss, remaining.current);
    return () => {
      clearTimeout(t);
      remaining.current -= Date.now() - started.current;
    };
  }, [paused, persistent, onDismiss]);
  return (
    <div
      role={tone === "error" ? "alert" : "status"}
      aria-live={tone === "error" ? "assertive" : "polite"}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      className="tn-toast relative flex w-[360px] gap-3 overflow-hidden rounded-md border border-border bg-surface-3 py-3 pl-4 pr-2 shadow-3"
    >
      <span aria-hidden="true" className={cx("absolute inset-y-0 left-0 w-[3px]", BAR[tone])} />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <p className="text-base font-medium text-fg">{item.title}</p>
        {item.body && <p className="text-sm text-fg-muted">{item.body}</p>}
        {item.action && (
          <div className="mt-1">
            <Button size="sm" variant="secondary" onClick={() => { item.action?.onClick(); onDismiss(); }}>
              {item.action.label}
            </Button>
          </div>
        )}
      </div>
      <button
        type="button"
        aria-label={dismissLabel}
        onClick={onDismiss}
        className="inline-flex size-6 shrink-0 items-center justify-center rounded-xs text-fg-subtle hover:bg-surface-4 hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
      >
        <X size={14} aria-hidden="true" />
      </button>
    </div>
  );
}

/** UI_SPEC §2.12: bottom-right, max 3 stacked, older ones collapse to "+n more".
 * Bottom-left while `data-toasts="left"` is on the root (a page of the
 * app's browser covers the corner, base.css). */
export function ToastProvider({
  children,
  moreLabel = (n: number) => `+${n} more`,
  dismissLabel = "Dismiss",
}: {
  children: ReactNode;
  moreLabel?: (n: number) => string;
  dismissLabel?: string;
}) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const next = useRef(1);
  const dismiss = useCallback((id: number) => setItems((all) => all.filter((t) => t.id !== id)), []);
  const show = useCallback((t: ToastInput) => {
    const id = next.current++;
    setItems((all) => [...all, { ...t, id }]);
    return id;
  }, []);
  const value = useMemo(() => ({ show, dismiss }), [show, dismiss]);
  const visible = items.slice(-3);
  const hidden = items.length - visible.length;
  return (
    <ToastContext.Provider value={value}>
      {children}
      <div aria-label="Notifications" role="region" data-toast-region="" className="pointer-events-none fixed bottom-8 right-4 z-[var(--z-toast)] flex flex-col items-end gap-2 [&>*]:pointer-events-auto">
        {hidden > 0 && <span className="rounded-full bg-surface-3 px-2 py-0.5 text-2xs text-fg-muted shadow-1">{moreLabel(hidden)}</span>}
        {visible.map((t) => (
          <ToastCard key={t.id} item={t} onDismiss={() => dismiss(t.id)} dismissLabel={dismissLabel} />
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): Ctx {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToast needs a ToastProvider");
  return ctx;
}
