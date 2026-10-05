import { Button, KeyValue, SettingsGroup, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useSetting } from "../../stores/settings";
import { copyDiagnostics } from "../shell/actions";
import { useBoot } from "../shell/useBoot";

/** "What leaves this machine" (F-TL-07): rows update when toggles change. */
export function PrivacyFlows() {
  const usage = useSetting("telemetry_usage", false);
  const crash = useSetting("telemetry_crash", false);
  const auto = useSetting("auto_consent", false);
  const hints = useSetting("dictate_share_hints", false);
  const rows = [
    { feature: t("settings.flows.usage"), on: usage, sends: t("settings.flows.usage.sends"), where: t("settings.flows.usage.where") },
    { feature: t("settings.flows.crash"), on: crash, sends: t("settings.flows.crash.sends"), where: t("settings.flows.crash.where") },
    { feature: t("settings.flows.auto"), on: auto, sends: t("settings.flows.auto.sends"), where: t("settings.flows.auto.where") },
    { feature: t("settings.flows.hints"), on: hints, sends: t("settings.flows.hints.sends"), where: t("settings.flows.hints.where") },
  ];
  return (
    <section className="flex flex-col gap-1.5">
      <h2 className="px-3 text-2xs font-semibold uppercase tracking-caps text-fg-subtle">{t("settings.flows")}</h2>
      <div className="overflow-hidden rounded-md border border-border bg-surface-2">
        <table className="w-full text-left text-sm">
          <thead className="text-fg-subtle">
            <tr>
              <th className="px-3 py-2 font-medium">{t("settings.flows.feature")}</th>
              <th className="px-3 py-2 font-medium">{t("settings.flows.status")}</th>
              <th className="px-3 py-2 font-medium">{t("settings.flows.sends")}</th>
              <th className="px-3 py-2 font-medium">{t("settings.flows.where")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.feature} className="border-t border-divider">
                <td className="px-3 py-2 text-fg">{r.feature}</td>
                <td className={r.on ? "px-3 py-2 text-fg" : "px-3 py-2 text-fg-muted"}>{r.on ? t("settings.flows.on") : t("settings.flows.off")}</td>
                <td className="px-3 py-2 text-fg-muted">{r.sends}</td>
                <td className="px-3 py-2 text-fg-muted">{r.where}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

/** UI-116 actions: diagnostics, logs, data folder, version, dev links. */
export function AdvancedExtras() {
  const boot = useBoot((s) => s.boot);
  const { show } = useToast();
  return (
    <SettingsGroup>
      <div className="flex flex-col gap-4 p-3">
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => void copyDiagnostics(show)}>{t("settings.copyDiagnostics")}</Button>
          <Button onClick={() => invoke("app_open_logs_folder").catch(() => undefined)}>{t("settings.openLogs")}</Button>
          {boot?.dev && (
            <Button variant="ghost" onClick={() => (window.location.hash = "#/__gallery")}>
              {t("settings.gallery")}
            </Button>
          )}
        </div>
        <KeyValue
          items={[
            { key: t("settings.dataFolder"), value: <code className="font-mono text-sm">{boot?.dataDirDisplay ?? ""}</code> },
            { key: t("settings.version"), value: t("settings.versionValue", { version: boot?.version ?? "", build: boot?.build ?? "" }) },
          ]}
        />
      </div>
    </SettingsGroup>
  );
}
