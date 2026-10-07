import { useEffect } from "react";
import type { DeepLink } from "@tandem/protocol";
import { useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { addWorkspace } from "./actions";

/** Handles `tandem://` links the core routed to the UI (F-SH-13). */
export function DeepLinks() {
  const { show } = useToast();
  useEffect(() => {
    const handle = (link: DeepLink) => {
      if (link.route === "open" && link.params.path) void addWorkspace(link.params.path, show);
      else show({ tone: "warning", title: t("deeplink.unrecognized") });
    };
    let off: (() => void) | undefined;
    void listen<DeepLink>("app:deep-link", handle).then((f) => (off = f));
    invoke<DeepLink[]>("deeplink_pending")
      .then((links) => links.forEach(handle))
      .catch(() => undefined);
    return () => off?.();
  }, [show]);
  return null;
}
