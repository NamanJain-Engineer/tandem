import type { ToastAction } from "@tandem/protocol";
import { Button } from "@tandem/ui";
import { t } from "../../i18n";
import { emitTo } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";

/** Runs a Dictate toast's action (F-DI-05). "See plans" opens the upgrade
 * sheet in the main window, wherever it was clicked. */
export function runToastAction(action: ToastAction): void {
  if (action === "use_local") void invoke<boolean>("dictate_use_local").catch(() => false);
  else void emitTo("main", "ui:open-plans", null);
}

export function toastActionLabel(action: ToastAction): string {
  return action === "use_local" ? t("dictate.action.useLocal") : t("dictate.action.seePlans");
}

/** The buttons under a toast ("Use local for this take" · "See plans"). */
export function ToastActions({ actions, onDone }: { actions: ToastAction[]; onDone?: () => void }) {
  if (actions.length === 0) return null;
  return (
    <span className="flex shrink-0 items-center gap-1" data-testid="toast-actions">
      {actions.map((a) => (
        <Button
          key={a}
          size="sm"
          variant={a === "use_local" ? "secondary" : "ghost"}
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
