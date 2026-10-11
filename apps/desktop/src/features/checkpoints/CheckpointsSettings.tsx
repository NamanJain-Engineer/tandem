// Settings › Checkpoints (UI-117, F-EX-01): the space checkpoints take and
// "Delete all checkpoints".
import { useEffect, useState } from "react";
import type { CheckpointUsage } from "@tandem/protocol";
import { Button, ConfirmDialog, KeyValue, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";

/** "412 MB", "3.1 GB", "0 KB". */
export function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.round(bytes / 1024)} KB`;
}

export function CheckpointsSettingsExtra() {
  const [usage, setUsage] = useState<CheckpointUsage | null>(null);
  const [asking, setAsking] = useState(false);
  const { show } = useToast();
  const load = () => void invoke<CheckpointUsage>("checkpoints_usage").then(setUsage, () => setUsage(null));
  useEffect(load, []);
  return (
    <div className="mt-4 space-y-3" data-testid="checkpoints-settings">
      <KeyValue
        items={[
          {
            key: t("settings.checkpoints.used"),
            value: usage ? t("settings.checkpoints.usedValue", { size: formatBytes(usage.bytes), n: usage.folders }) : "…",
          },
        ]}
      />
      <Button variant="danger" size="sm" onClick={() => setAsking(true)} data-testid="checkpoints-delete-all">
        {t("settings.checkpoints.deleteAll")}
      </Button>
      <ConfirmDialog
        open={asking}
        onOpenChange={setAsking}
        danger
        title={t("settings.checkpoints.deleteAll.title")}
        body={t("settings.checkpoints.deleteAll.body")}
        confirmLabel={t("settings.checkpoints.deleteAll.confirm")}
        onConfirm={() => {
          setAsking(false);
          void invoke("checkpoints_delete_all", {}, { timeoutMs: 120_000 }).then(
            () => {
              show({ tone: "success", title: t("settings.checkpoints.deleted") });
              load();
            },
            (e: { message?: string }) => show({ tone: "error", title: e.message ?? "" }),
          );
        }}
      />
    </div>
  );
}
