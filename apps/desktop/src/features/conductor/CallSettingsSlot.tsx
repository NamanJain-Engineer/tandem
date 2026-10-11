import { Select, SettingsRow, Switch } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import { useSession } from "../../stores/session";
import { useSettings } from "../../stores/settings";

const KEEP = ["off", "7", "30"] as const;

/** Conductor's call rows in Settings › Conductor (UI-67): the floating
 * ring, and how long the call log is kept on this computer. F-CN-06's page renders
 * this first among its extras. Absent on plans without Conductor. */
export function ConductorCallSettings() {
  const enabled = useSession((s) => !!s.entitlement?.limits.conductor);
  const value = useSettings((s) => s.values.conductor_call_logs);
  const ring = useSettings((s) => s.values.conductor_floating_ring === true);
  const set = useSettings((s) => s.set);
  if (!enabled) return null;
  const current = typeof value === "string" && (KEEP as readonly string[]).includes(value) ? value : "7";
  return (
    <>
      <SettingsRow
        settingKey="conductor_floating_ring"
        label={t("conductor.settings.floatingRing")}
        description={t("conductor.settings.floatingRingHint")}
        control={({ id }) => (
          <Switch
            id={id}
            aria-label={t("conductor.settings.floatingRing")}
            checked={ring}
            onCheckedChange={(on) => void set("conductor_floating_ring", on)}
          />
        )}
      />
      <SettingsRow
        settingKey="conductor_call_logs"
        label={t("conductor.settings.keepLogs")}
        description={t("conductor.settings.keepLogsHint")}
        control={({ id }) => (
          <Select
            id={id}
            ariaLabel={t("conductor.settings.keepLogs")}
            value={current}
            onValueChange={(v) => void set("conductor_call_logs", v)}
            options={KEEP.map((k) => ({ value: k, label: t(`conductor.settings.keep.${k}` as MessageKey) }))}
            size="sm"
          />
        )}
      />
    </>
  );
}
