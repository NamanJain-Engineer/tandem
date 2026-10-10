import { AlertDialog as A, Dialog as D } from "radix-ui";
import { X } from "lucide-react";
import { useState, type KeyboardEvent, type ReactNode } from "react";
import { cx } from "../cx";
import { Button } from "./Button";
import { Input } from "./Field";

const WIDTH = { sm: "w-[400px]", md: "w-[520px]", lg: "w-[720px]" };
const OVERLAY = "tn-fade fixed inset-0 z-[var(--z-dialog)] bg-overlay";
const PANEL =
  "tn-dialog fixed left-1/2 top-1/2 z-[var(--z-dialog)] flex max-h-[85vh] max-w-[calc(100vw-32px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-lg border border-border bg-surface-3 shadow-3 focus:outline-none";

export type DialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  size?: keyof typeof WIDTH;
  footer?: ReactNode;
  /** False for crash restore and forced update: Esc and outside clicks do nothing. */
  dismissable?: boolean;
  closeLabel?: string;
  children?: ReactNode;
};

/** UI_SPEC §2.10: centred, focus trapped, returns focus to the trigger. */
export function Dialog({ open, onOpenChange, title, description, size = "md", footer, dismissable = true, closeLabel = "Close", children }: DialogProps) {
  return (
    <D.Root open={open} onOpenChange={(o) => (dismissable || o ? onOpenChange(o) : undefined)}>
      <D.Portal>
        <D.Overlay className={OVERLAY} />
        <D.Content
          className={cx(PANEL, WIDTH[size])}
          onEscapeKeyDown={(e) => !dismissable && e.preventDefault()}
          onPointerDownOutside={(e) => !dismissable && e.preventDefault()}
          {...(description ? {} : { "aria-describedby": undefined })}
        >
          <div className="flex items-start gap-3 px-6 pt-6">
            <div className="flex min-w-0 flex-1 flex-col gap-1">
              <D.Title className="text-lg font-semibold text-fg">{title}</D.Title>
              {description && <D.Description className="text-base text-fg-muted">{description}</D.Description>}
            </div>
            {dismissable && (
              <D.Close
                aria-label={closeLabel}
                className="-mr-2 -mt-1 inline-flex size-7 items-center justify-center rounded-sm text-fg-muted hover:bg-surface-4 hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
              >
                <X size={16} aria-hidden="true" />
              </D.Close>
            )}
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">{children}</div>
          {footer && <div className="flex items-center justify-end gap-2 px-6 pb-6">{footer}</div>}
        </D.Content>
      </D.Portal>
    </D.Root>
  );
}

export type ConfirmDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  body?: ReactNode;
  confirmLabel: string;
  cancelLabel?: string;
  danger?: boolean;
  /** Typed confirmation, e.g. the branch name or "delete". */
  requireText?: string;
  requireLabel?: (text: string) => string;
  onConfirm: () => void;
  /** Extra controls (a checkbox, say) between the body and the typed confirmation. */
  children?: ReactNode;
};

/** UI_SPEC §2.10 / UI-135. Danger confirms are `alertdialog` with focus on
 * Cancel; others focus the confirm button. Mod+Enter confirms when allowed. */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  body,
  confirmLabel,
  cancelLabel = "Cancel",
  danger,
  requireText,
  requireLabel = (t) => `Type "${t}" to confirm`,
  onConfirm,
  children,
}: ConfirmDialogProps) {
  const [typed, setTyped] = useState("");
  const allowed = !requireText || typed === requireText;
  const confirm = () => {
    if (!allowed) return;
    onConfirm();
    onOpenChange(false);
    setTyped("");
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      confirm();
    }
  };
  return (
    <A.Root open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) setTyped(""); }}>
      <A.Portal>
        <A.Overlay className={OVERLAY} />
        <A.Content className={cx(PANEL, WIDTH.sm, "gap-4 p-6")} onKeyDown={onKeyDown} role={danger ? "alertdialog" : "dialog"}>
          <A.Title className="text-lg font-semibold text-fg">{title}</A.Title>
          {body ? <A.Description className="text-base text-fg-muted">{body}</A.Description> : <A.Description className="sr-only">{confirmLabel}</A.Description>}
          {children}
          {requireText &&<Input label={requireLabel(requireText)} value={typed} onChange={(e) => setTyped(e.target.value)} autoComplete="off" />}
          <div className="flex justify-end gap-2">
            <A.Cancel asChild>
              <Button variant="ghost" autoFocus={danger}>
                {cancelLabel}
              </Button>
            </A.Cancel>
            <A.Action asChild>
              <Button
                variant={danger ? "danger" : "primary"}
                disabled={!allowed}
                autoFocus={!danger && !requireText}
                onClick={(e) => {
                  e.preventDefault();
                  confirm();
                }}
              >
                {confirmLabel}
              </Button>
            </A.Action>
          </div>
        </A.Content>
      </A.Portal>
    </A.Root>
  );
}

export type SheetProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  subtitle?: ReactNode;
  size?: "md" | "lg";
  footer?: ReactNode;
  /** Esc asks "Discard changes?" when the form is dirty. */
  dirty?: boolean;
  /** Non-blocking sheets keep the page interactive (aria-modal=false). */
  modal?: boolean;
  closeLabel?: string;
  discardPrompt?: { title: string; confirm: string; cancel: string };
  children?: ReactNode;
};

/** UI_SPEC §2.11: right-side panel below the title bar. */
export function Sheet({
  open,
  onOpenChange,
  title,
  subtitle,
  size = "md",
  footer,
  dirty,
  modal = true,
  closeLabel = "Close",
  discardPrompt = { title: "Discard changes?", confirm: "Discard", cancel: "Keep editing" },
  children,
}: SheetProps) {
  const [asking, setAsking] = useState(false);
  const requestClose = () => (dirty ? setAsking(true) : onOpenChange(false));
  return (
    <>
      <D.Root open={open} modal={modal} onOpenChange={(o) => (o ? onOpenChange(true) : requestClose())}>
        <D.Portal>
          {modal && <D.Overlay className={OVERLAY.replace("z-[var(--z-dialog)]", "z-[var(--z-sheet)]")} />}
          <D.Content
            aria-modal={modal}
            className={cx(
              "tn-sheet fixed bottom-0 right-0 top-[var(--titlebar-h)] z-[var(--z-sheet)] flex flex-col border-l border-border bg-surface-3 shadow-3 focus:outline-none",
              size === "md" ? "w-[480px]" : "w-[640px]",
              "max-w-full",
            )}
          >
            <div className="flex items-start gap-3 border-b border-divider px-4 py-3">
              <div className="flex min-w-0 flex-1 flex-col">
                <D.Title className="text-lg font-semibold text-fg">{title}</D.Title>
                {subtitle ? <D.Description className="text-sm text-fg-muted">{subtitle}</D.Description> : <D.Description className="sr-only">{title}</D.Description>}
              </div>
              <button
                type="button"
                aria-label={closeLabel}
                onClick={requestClose}
                className="inline-flex size-7 items-center justify-center rounded-sm text-fg-muted hover:bg-surface-4 hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
              >
                <X size={16} aria-hidden="true" />
              </button>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto p-4">{children}</div>
            {footer && <div className="flex items-center justify-end gap-2 border-t border-divider px-4 py-3">{footer}</div>}
          </D.Content>
        </D.Portal>
      </D.Root>
      <ConfirmDialog
        open={asking}
        onOpenChange={setAsking}
        title={discardPrompt.title}
        confirmLabel={discardPrompt.confirm}
        cancelLabel={discardPrompt.cancel}
        danger
        onConfirm={() => onOpenChange(false)}
      />
    </>
  );
}
