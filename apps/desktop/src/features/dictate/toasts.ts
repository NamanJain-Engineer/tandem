import type { DictateToast } from "@tandem/protocol";
import { isMac } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";

/** The one-line text of a Dictate toast (D-505): the core sends a code, the
 * words live in the catalog. */
export function toastText(toast: DictateToast): string {
  const key = `dictate.toast.${toast.code}` as MessageKey;
  return t(key, {
    app: toast.app ?? "",
    model: toast.model ?? "",
    previous: toast.previous ?? "",
    mod: isMac() ? "⌘" : "Ctrl+",
  });
}

/** Toasts the main window shows itself (the widget shows the rest). */
export function mainWindowShows(toast: DictateToast, lastTakeWasComposer: boolean, widgetHidden: boolean): boolean {
  if (toast.code === "model_failed" || toast.code === "model_fallback") return true;
  return lastTakeWasComposer || widgetHidden;
}
