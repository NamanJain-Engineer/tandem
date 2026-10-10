import { useState } from "react";
import type { DataExport, IpcError } from "@tandem/protocol";
import { Button, Checkbox, ConfirmDialog, SettingsGroup, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useSession } from "../../stores/session";
import { openBilling } from "../shell/actions";

/** The export waits on the save dialog, then zips everything: give it time. */
const EXPORT_TIMEOUT_MS = 10 * 60_000;

/** UI-112 "Your data" (F-TL-05): export, delete local data, delete account. */
export function DataControls() {
  const { show } = useToast();
  const auth = useSession((s) => s.auth);
  const [exporting, setExporting] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [engines, setEngines] = useState(false);

  const exportData = async () => {
    setExporting(true);
    try {
      const out = await invoke<DataExport | null>("data_export", undefined, { timeoutMs: EXPORT_TIMEOUT_MS });
      if (out) show({ tone: "success", title: t("settings.data.exported"), body: out.path });
    } catch (e) {
      show({ tone: "error", title: t("settings.data.exportFailed"), body: (e as IpcError).message });
    } finally {
      setExporting(false);
    }
  };

  // The app relaunches into a first run; under the test harness it stays.
  const deleteData = async (removeEngineSignIns: boolean) => {
    try {
      await invoke("data_delete_local", { removeEngineSignIns }, { timeoutMs: 60_000 });
    } catch (e) {
      show({ tone: "error", title: t("settings.data.deleteFailed"), body: (e as IpcError).message });
    }
  };

  return (
    <section className="flex flex-col gap-1.5" data-testid="data-controls">
      <h2 className="px-3 text-2xs font-semibold uppercase tracking-caps text-fg-subtle">{t("settings.data")}</h2>
      <SettingsGroup>
        <Action description={t("settings.data.exportDesc")}>
          <Button onClick={() => void exportData()} disabled={exporting} data-testid="data-export">
            {exporting ? t("settings.data.exporting") : t("settings.data.export")}
          </Button>
        </Action>
        <Action description={t("settings.data.deleteDesc")}>
          <Button
            variant="danger"
            data-testid="data-delete"
            onClick={() => {
              setEngines(false);
              setConfirming(true);
            }}
          >
            {t("settings.data.delete")}
          </Button>
        </Action>
        {auth?.signedIn && auth.hosted && (
          <Action description={t("settings.data.deleteAccountDesc")}>
            <Button variant="ghost" onClick={() => void openBilling({ type: "account" }, show)}>
              {t("settings.data.deleteAccount")}
            </Button>
          </Action>
        )}
      </SettingsGroup>
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        title={t("settings.data.deleteTitle")}
        body={engines ? t("settings.data.deleteBodyEngines") : t("settings.data.deleteBody")}
        confirmLabel={t("settings.data.deleteConfirm")}
        danger
        requireText="delete"
        onConfirm={() => void deleteData(engines)}
      >
        <Checkbox label={t("settings.data.deleteEngines")} checked={engines} onCheckedChange={setEngines} />
      </ConfirmDialog>
    </section>
  );
}

function Action({ description, children }: { description: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 p-3">
      <p className="max-w-xl text-sm text-fg-muted">{description}</p>
      {children}
    </div>
  );
}
