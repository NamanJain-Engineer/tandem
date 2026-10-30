import { Cpu } from "lucide-react";
import { Tooltip } from "@tandem/ui";
import { t } from "../../i18n";

/** UI-164 (F-EN-11): a Codex pane started on a model server on this
 * computer. It runs on no account, so this stands where the account chip
 * would; the tooltip names the model and server. */
export function LocalModelChip({ label }: { label: string }) {
  return (
    <Tooltip label={label}>
      <span
        data-testid="local-model-chip"
        className="inline-flex h-5 shrink-0 items-center gap-1 rounded-full border border-border bg-surface-3 px-1.5 text-xs text-fg-muted"
      >
        <Cpu size={11} aria-hidden="true" className="shrink-0" />
        <span>{t("local.chip")}</span>
      </span>
    </Tooltip>
  );
}
