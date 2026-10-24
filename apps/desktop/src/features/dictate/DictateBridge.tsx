import { useEffect, useRef } from "react";
import type { DictateStateEvent, DictateToast } from "@tandem/protocol";
import { useToast } from "@tandem/ui";
import { listen } from "../../ipc/events";
import { useDictate } from "../../stores/dictate";
import { useSetting } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { runToastAction, toastActionLabel } from "./ToastActions";
import { mainWindowShows, toastText } from "./toasts";

/** The main window's side of Dictate: its toasts (composer takes, model
 * changes, or every one while the widget is hidden) and the tray's
 * "open Settings › Dictate". */
export function DictateBridge() {
  const { show } = useToast();
  const visibility = useSetting<string>("dictate_widget_visibility", "while_recording");
  const lastComposer = useRef(false);
  const hidden = useRef(visibility === "hidden");
  hidden.current = visibility === "hidden";
  useEffect(() => {
    void useDictate.getState().loadStatus();
    const offs = [
      listen<DictateStateEvent>("dictate:state", (e) => {
        if (e.state === "listening") lastComposer.current = e.composer !== null;
      }),
      listen<DictateToast>("dictate:toast", (toast) => {
        if (!mainWindowShows(toast, lastComposer.current, hidden.current)) return;
        const action = toast.actions[0];
        show({
          tone: toast.error ? "error" : "info",
          title: toastText(toast),
          body: toast.detail || undefined,
          action: action ? { label: toastActionLabel(action), onClick: () => runToastAction(action) } : undefined,
        });
      }),
      listen<{ page: string }>("ui:open-settings", (e) => useUi.getState().openSettings(e.page)),
      // Verified anywhere (Try it, or the fallback link on Wayland): the home card updates.
      listen<null>("dictate:verified", () => void useDictate.getState().loadStatus()),
    ];
    return () => offs.forEach((o) => void o.then((off) => off()));
  }, [show]);
  return null;
}
