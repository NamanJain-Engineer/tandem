import { useEffect } from "react";
import { Skeleton } from "@tandem/ui";
import { t } from "../../i18n";
import { useResources } from "../../stores/resources";
import { SelectControl } from "./controls";

type Ids = { id: string; describedBy?: string };

const CAPS: Array<[string, string]> = [
  ["512", "512 MB"],
  ["1024", "1 GB"],
  ["2048", "2 GB"],
  ["4096", "4 GB"],
  ["8192", "8 GB"],
  ["16384", "16 GB"],
];

/** Settings › Engines › Memory cap per agent (F-EX-06): a cap only where it applies. */
export function MemoryCapSetting({ ids }: { ids: Ids }) {
  const status = useResources((s) => s.status);
  const state = useResources((s) => s.statusState);
  useEffect(() => {
    if (useResources.getState().statusState === "idle") void useResources.getState().loadStatus();
  }, []);
  const unavailable = state === "ready" && !status?.capping;
  return (
    <span className="flex flex-col items-end gap-1" data-testid="memory-cap-setting">
      <SelectControl
        k="agent_memory_cap"
        ids={ids}
        fallback="off"
        options={[
          { value: "off", label: t("resources.cap.off") },
          ...CAPS.map(([value, label]) => ({
            value,
            label,
            // Never set where it can't apply: nobody thinks they're capped.
            disabled: unavailable,
            disabledReason: status?.reason ?? t("resources.cap.unavailable"),
          })),
        ]}
      />
      {state === "loading" || state === "idle" ? (
        <Skeleton width={160} />
      ) : state === "error" ? (
        <span className="text-xs text-danger">{t("resources.cap.statusError")}</span>
      ) : status?.capping ? (
        <span className="text-xs text-fg-muted" data-testid="memory-cap-status" data-available="true">
          {t("resources.cap.ok")}
        </span>
      ) : (
        <span className="text-xs text-warning-text" data-testid="memory-cap-status" data-available="false">
          {status?.reason ?? t("resources.cap.unavailable")}
        </span>
      )}
    </span>
  );
}
