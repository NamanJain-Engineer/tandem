import { useEffect, useState } from "react";
import { Button, Callout, Dialog, KeyValue, SettingsGroup, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useSession } from "../../stores/session";
import { useSettings } from "../../stores/settings";
import { copyDiagnostics } from "../shell/actions";
import { useBoot } from "../shell/useBoot";
import { DataControls } from "./DataControls";
import { type FlowRow, flowRows } from "./flows";

/** "What leaves this machine" (F-TL-07): built from hosted-features.json,
 * each row live from settings, sign-in and the Team org's policy. */
export function PrivacyFlows() {
  const values = useSettings((s) => s.values);
  const entitlement = useSession((s) => s.entitlement);
  const signedIn = useSession((s) => s.auth?.signedIn ?? false);
  const rows = flowRows(values, entitlement, signedIn);
  const status = (r: FlowRow) =>
    r.status === "on" ? t("settings.flows.on") : r.status === "on_use" ? t("settings.flows.onUse") : t("settings.flows.off");
  return (
    <section className="flex flex-col gap-1.5">
      {entitlement?.policy?.auditEnabled && (
        <Callout tone="accent">
          <span data-testid="audit-notice">{t("settings.auditNotice")}</span>
        </Callout>
      )}
      <h2 className="px-3 text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
        {t("settings.flows")}
      </h2>
      <div className="overflow-hidden rounded-md border border-border bg-surface-2">
        <table className="w-full text-left text-sm" data-testid="privacy-flows">
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
              <tr key={r.id} data-flow={r.id} data-status={r.status} className="border-t border-divider">
                <td className="px-3 py-2 text-fg">{r.label}</td>
                <td className={r.status === "on" ? "px-3 py-2 text-fg" : "px-3 py-2 text-fg-muted"}>{status(r)}</td>
                <td className="px-3 py-2 text-fg-muted">{r.sends}</td>
                <td className="px-3 py-2 text-fg-muted">{r.destination}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <DataControls />
    </section>
  );
}

/** UI-116 actions: diagnostics, logs, data folder, version, dev links. */
/** About › Licenses (F-TL-04 AC2): the notices this build carries. */
function LicensesDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const [text, setText] = useState<string | null>(null);
  useEffect(() => {
    if (!open || text !== null) return;
    void invoke<string>("app_licenses")
      .then(setText)
      .catch(() => setText(""));
  }, [open, text]);
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t("settings.licenses")} size="lg">
      <pre
        data-testid="licenses-text"
        tabIndex={0}
        className="max-h-[60vh] overflow-auto whitespace-pre-wrap rounded-sm bg-surface-1 p-3 font-mono text-xs text-fg focus-visible:shadow-focus focus-visible:outline-none"
      >
        {text ?? t("settings.licensesLoading")}
      </pre>
    </Dialog>
  );
}

export function AdvancedExtras() {
  const boot = useBoot((s) => s.boot);
  const { show } = useToast();
  const [licenses, setLicenses] = useState(false);
  return (
    <SettingsGroup>
      <LicensesDialog open={licenses} onOpenChange={setLicenses} />
      <div className="flex flex-col gap-4 p-3">
        <div className="flex flex-wrap gap-2">
          <Button onClick={() => void copyDiagnostics(show)}>{t("settings.copyDiagnostics")}</Button>
          <Button data-testid="open-licenses" onClick={() => setLicenses(true)}>
            {t("settings.licenses")}
          </Button>
          <Button onClick={() => invoke("app_open_logs_folder").catch(() => undefined)}>
            {t("settings.openLogs")}
          </Button>
          {boot?.dev && (
            <Button variant="ghost" onClick={() => (window.location.hash = "#/__gallery")}>
              {t("settings.gallery")}
            </Button>
          )}
        </div>
        <KeyValue
          items={[
            {
              key: t("settings.dataFolder"),
              value: <code className="font-mono text-sm">{boot?.dataDirDisplay ?? ""}</code>,
            },
            {
              key: t("settings.version"),
              value: (
                <span className="flex flex-col">
                  <span>{t("settings.versionValue", { version: boot?.version ?? "", build: boot?.build ?? "" })}</span>
                  {/* DESIGN §5.5: the one quiet line, under the version. */}
                  <span className="text-sm text-fg-muted" data-testid="settings-free-line">
                    {t("settings.freeLine")}
                  </span>
                </span>
              ),
            },
          ]}
        />
      </div>
    </SettingsGroup>
  );
}
