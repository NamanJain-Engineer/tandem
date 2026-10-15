import { useEffect, useState } from "react";
import type { GlobeKeyView, GlobeUsage, IpcError } from "@tandem/protocol";
import { Button, ConfirmDialog, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";

function usageLabel(u: GlobeUsage | null): string {
  switch (u) {
    case "do_nothing":
      return t("dictate.globe.usage.doNothing");
    case "change_input_source":
      return t("dictate.globe.usage.inputSource");
    case "show_emoji":
      return t("dictate.globe.usage.emoji");
    case "start_dictation":
      return t("dictate.globe.usage.dictation");
    default:
      return t("dictate.globe.usage.default");
  }
}

/** macOS's "Press 🌐 key to" (F-DI-11, D-524): with Fn as the Dictate key,
 * a 🌐 press can also open the emoji picker or switch input sources. The
 * app changes it to Do Nothing only after the dialog, and can undo it. */
export function GlobeKey() {
  const { show } = useToast();
  const [view, setView] = useState<GlobeKeyView | null>(null);
  const [ask, setAsk] = useState(false);
  useEffect(() => {
    void invoke<GlobeKeyView>("dictate_globe_key").then(setView, () => undefined);
  }, []);
  if (!view?.available) return null;
  const set = (doNothing: boolean) =>
    void invoke<GlobeKeyView>("dictate_globe_key_set", { doNothing }).then(setView, (e: IpcError) =>
      show({ tone: "error", title: e.message }),
    );
  return (
    <div className="flex flex-wrap items-center gap-2 text-sm" data-testid="dictate-globe" data-usage={view.usage ?? "default"}>
      <span className="text-fg-muted">{t("dictate.globe.now", { usage: usageLabel(view.usage) })}</span>
      {view.changedByApp ? (
        <Button size="sm" variant="ghost" onClick={() => set(false)} data-testid="dictate-globe-undo">
          {t("dictate.globe.undo")}
        </Button>
      ) : view.usage !== "do_nothing" ? (
        <Button size="sm" variant="secondary" onClick={() => setAsk(true)} data-testid="dictate-globe-change">
          {t("dictate.globe.change")}
        </Button>
      ) : null}
      <ConfirmDialog
        open={ask}
        onOpenChange={setAsk}
        title={t("dictate.globe.consentTitle")}
        body={t("dictate.globe.consentBody", { usage: usageLabel(view.usage) })}
        confirmLabel={t("dictate.globe.consentConfirm")}
        onConfirm={() => set(true)}
      />
    </div>
  );
}
