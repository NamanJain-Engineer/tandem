import type { ToastAction } from "@tandem/protocol";
import { Button } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { invoke } from "../../ipc/invoke";

/** Runs a Dictate toast's action (F-DI-05). */
export function runToastAction(action: ToastAction): void {
  if (action === "use_local") void invoke<boolean>("dictate_use_local").catch(() => false);
}

const LABELS: Record<ToastAction, MessageKey> = { use_local: "dictate.action.useLocal" };

export function toastActionLabel(action: ToastAction): string {
  return t(LABELS[action]);
}

/** The buttons under a toast ("Use local for this take"). */
export function ToastActions({ actions, onDone }: { actions: ToastAction[]; onDone?: () => void }) {
  if (actions.length === 0) return null;
  return (
    <span className="flex shrink-0 items-center gap-1" data-testid="toast-actions">
      {actions.map((a) => (
        <Button
          key={a}
          size="sm"
          variant="secondary"
          data-testid={`toast-action-${a}`}
          onClick={() => {
            runToastAction(a);
            onDone?.();
          }}
        >
          {toastActionLabel(a)}
        </Button>
      ))}
    </span>
  );
}
