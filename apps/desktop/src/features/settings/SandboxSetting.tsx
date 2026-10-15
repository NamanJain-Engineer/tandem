import { useEffect } from "react";
import { Skeleton } from "@tandem/ui";
import { t } from "../../i18n";
import { useSandbox } from "../../stores/sandbox";
import { SelectControl } from "./controls";

type Ids = { id: string; describedBy?: string };

/** Settings › Engines › Sandbox agent panes (F-TL-11): the choice and whether it can run here. */
export function SandboxSetting({ ids }: { ids: Ids }) {
  const status = useSandbox((s) => s.status);
  const state = useSandbox((s) => s.statusState);
  useEffect(() => {
    if (useSandbox.getState().statusState === "idle") void useSandbox.getState().loadStatus();
  }, []);
  return (
    <span className="flex flex-col items-end gap-1" data-testid="sandbox-setting">
      <SelectControl
        k="agent_sandbox"
        ids={ids}
        fallback="off"
        options={[
          { value: "off", label: t("sandbox.off") },
          { value: "workspace", label: t("sandbox.workspace") },
        ]}
      />
      {state === "loading" || state === "idle" ? (
        <Skeleton width={160} />
      ) : state === "error" ? (
        <span className="text-xs text-danger">{t("sandbox.statusError")}</span>
      ) : status?.available ? (
        <span className="text-xs text-fg-muted" data-testid="sandbox-status" data-available="true">
          {t("sandbox.uses", { tool: status.tool ?? "bubblewrap" })}
        </span>
      ) : (
        <span className="text-xs text-warning-text" data-testid="sandbox-status" data-available="false">
          {status?.reason ?? t("sandbox.unavailable")}
        </span>
      )}
    </span>
  );
}
