import { Button, Tooltip } from "@tandem/ui";
import { t } from "../../i18n";
import { pillOf, useUpdates } from "../../stores/updates";

/** UI-130's title-bar pill: "Restart to update", or "Update failed — Retry"
 * with the failing stage in the tooltip. */
export function UpdatePill() {
  const state = useUpdates((s) => s.state);
  const pill = pillOf(state);
  if (!pill || !state) return null;
  if (pill === "ready")
    return (
      <Button
        size="sm"
        variant="primary"
        data-testid="update-pill"
        data-stage="ready"
        onClick={() => useUpdates.getState().setRestartOpen(true)}
      >
        {t("update.restart")}
      </Button>
    );
  const stage = state.errorStage ?? "download";
  return (
    <Tooltip label={t("update.failedAt", { stage: t(`update.stage.${stage}`) })}>
      <Button
        size="sm"
        variant="danger"
        data-testid="update-pill"
        data-stage={stage}
        onClick={() => void useUpdates.getState().checkNow()}
      >
        {t("update.failed")}
      </Button>
    </Tooltip>
  );
}
