import { useState } from "react";
import { Checkbox, ConfirmDialog, SegmentedControl } from "@tandem/ui";
import { t } from "../../i18n";
import { useSession } from "../../stores/session";
import { useSetting, useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";

/** "Transcribe with [Local | Cloud ◈ PRO]" (UI-62, F-DI-05). Cloud needs
 * Pro (Free opens the plans), and the first switch asks before any audio
 * is sent. With Cloud on, the dictionary hints opt-in shows below. */
export function EngineControl() {
  const engine = useSetting<string>("dictate_engine", "local");
  const consented = useSetting<boolean>("dictate_cloud_consented", false);
  const hints = useSetting<boolean>("dictate_share_hints", false);
  const plan = useSession((s) => s.entitlement?.plan ?? "free");
  const [ask, setAsk] = useState(false);
  const pro = plan !== "free";
  const set = (key: "dictate_engine" | "dictate_cloud_consented" | "dictate_share_hints", value: unknown) =>
    void useSettings.getState().set(key, value);
  const choose = (v: string) => {
    if (v === "local") set("dictate_engine", "local");
    else if (!pro) useUi.getState().openUpgrade();
    else if (!consented) setAsk(true);
    else set("dictate_engine", "cloud");
  };
  const cloud = engine === "cloud";
  return (
    <div className="flex flex-col items-end gap-1.5" data-testid="dictate-engine" data-engine={engine}>
      <SegmentedControl
        label={t("dictate.engine.label")}
        size="sm"
        value={cloud ? "cloud" : "local"}
        onValueChange={choose}
        items={[
          { id: "local", label: t("dictate.engine.local") },
          { id: "cloud", label: t("dictate.engine.cloud"), tooltip: pro ? undefined : t("dictate.engine.needsPro") },
        ]}
      />
      <span className="max-w-80 text-right text-sm text-fg-muted" data-testid="dictate-engine-help">
        {cloud ? t("dictate.engine.cloudHelp") : t("dictate.engine.localHelp")}
      </span>
      {cloud && (
        <Checkbox
          label={t("dictate.engine.shareHints")}
          checked={hints}
          onCheckedChange={(v) => set("dictate_share_hints", v)}
        />
      )}
      <ConfirmDialog
        open={ask}
        onOpenChange={setAsk}
        title={t("dictate.engine.consentTitle")}
        body={t("dictate.engine.consentBody")}
        confirmLabel={t("dictate.engine.consentConfirm")}
        onConfirm={() => {
          set("dictate_cloud_consented", true);
          set("dictate_engine", "cloud");
        }}
      />
    </div>
  );
}
