import { Shield, ShieldOff } from "lucide-react";
import { Tooltip } from "@tandem/ui";
import { t } from "../../i18n";
import { useSandbox } from "../../stores/sandbox";

const CHIP =
  "inline-flex h-5 shrink-0 items-center gap-1 rounded-full border border-border bg-surface-3 px-1.5 text-xs text-fg-muted";

/** F-TL-11: "Sandboxed" on an agent pane, or why it isn't. */
export function SandboxChip({ paneId }: { paneId: string }) {
  const note = useSandbox((s) => s.byPane[paneId]);
  if (!note) return null;
  const label = note.sandboxed ? t("sandbox.chip") : t("sandbox.chipOff");
  const tip = note.sandboxed ? t("sandbox.chip.tip") : (note.reason ?? t("sandbox.chipOff"));
  return (
    <Tooltip label={tip}>
      <span
        tabIndex={0}
        className={CHIP}
        data-testid="sandbox-chip"
        data-sandboxed={note.sandboxed ? "true" : "false"}
      >
        {note.sandboxed ? <Shield size={12} aria-hidden /> : <ShieldOff size={12} aria-hidden />}
        {label}
      </span>
    </Tooltip>
  );
}
