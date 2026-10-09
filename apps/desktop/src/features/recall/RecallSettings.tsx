// Settings › Terminal › Command recall (UI-141, F-EX-02): Clear command history.
import { useState } from "react";
import { Button, ConfirmDialog, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";

export function RecallSettingsExtra() {
  const [asking, setAsking] = useState(false);
  const { show } = useToast();
  return (
    <div className="mt-4" data-testid="recall-settings">
      <Button variant="danger" size="sm" data-testid="recall-clear" onClick={() => setAsking(true)}>
        {t("settings.recall.clear")}
      </Button>
      <ConfirmDialog
        open={asking}
        onOpenChange={setAsking}
        danger
        title={t("settings.recall.clear.title")}
        body={t("settings.recall.clear.body")}
        confirmLabel={t("settings.recall.clear.confirm")}
        onConfirm={() => {
          setAsking(false);
          void invoke<number>("history_clear").then(
            (n) => show({ tone: "success", title: t("settings.recall.cleared", { n }) }),
            (e: { message?: string }) => show({ tone: "error", title: e.message ?? "" }),
          );
        }}
      />
    </div>
  );
}
