import { Gauge, Snowflake } from "lucide-react";
import { Tooltip } from "@tandem/ui";
import { t } from "../../i18n";
import { useResources } from "../../stores/resources";

const CHIP =
  "inline-flex h-5 shrink-0 items-center gap-1 rounded-full border border-border bg-surface-3 px-1.5 text-xs text-fg-muted";

/** F-EX-06: "Frozen", "Stopped: memory cap 512 MB" or "Not capped" on a pane. */
export function ResourceChip({ paneId }: { paneId: string }) {
  const note = useResources((s) => s.byPane[paneId]);
  if (!note) return null;
  const chips = [];
  if (note.frozen)
    chips.push(
      <Tooltip key="frozen" label={t("resources.chip.frozen.tip")}>
        <span tabIndex={0} className={CHIP} data-testid="resource-chip" data-kind="frozen">
          <Snowflake size={12} aria-hidden />
          {t("resources.chip.frozen")}
        </span>
      </Tooltip>,
    );
  if (note.cappedAt)
    chips.push(
      <Tooltip key="capped" label={t("resources.chip.capped.tip")}>
        <span tabIndex={0} className={CHIP} data-testid="resource-chip" data-kind="capped">
          <Gauge size={12} aria-hidden />
          {t("resources.chip.capped", { cap: note.cappedAt })}
        </span>
      </Tooltip>,
    );
  else if (note.notCapped)
    chips.push(
      <Tooltip key="uncapped" label={note.notCapped}>
        <span tabIndex={0} className={CHIP} data-testid="resource-chip" data-kind="not-capped">
          <Gauge size={12} aria-hidden />
          {t("resources.chip.notCapped")}
        </span>
      </Tooltip>,
    );
  return <>{chips}</>;
}
