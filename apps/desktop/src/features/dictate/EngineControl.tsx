import { useState } from "react";
import { Checkbox, ConfirmDialog, SegmentedControl } from "@tandem/ui";
import { t } from "../../i18n";
import { useSession } from "../../stores/session";
import { useSetting, useSettings } from "../../stores/settings";

/** "Transcribe with [Local | Cloud]" (UI-62, F-DI-05). Cloud is hosted: it
 * comes with Vibezeo accounts and stays put until the limits carry it
 * (D-2903), and the first switch asks before any audio is sent. With Cloud
 * on, the dictionary hints opt-in shows below. */
export function EngineControl() {
  const engine = useSetting<string>("dictate_engine", "local");
  const consented = useSetting<boolean>("dictate_cloud_consented", false);
  const hints = useSetting<boolean>("dictate_share_hints", false);
  const hosted = useSession((s) => s.entitlement?.limits.cloudStt ?? false);
  const [ask, setAsk] = useState(false);
  const set = (key: "dictate_engine" | "dictate_cloud_consented" | "dictate_share_hints", value: unknown) =>
    void useSettings.getState().set(key, value);
  const choose = (v: string) => {
    if (v === "local") set("dictate_engine", "local");
    else if (!hosted) return;
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
          { id: "cloud", label: t("dictate.engine.cloud"), tooltip: hosted ? undefined : t("dictate.engine.needsPro") },
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
