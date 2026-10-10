import { useState } from "react";
import type { PluginApprovalDetail } from "@tandem/protocol";
import { Badge, Button, CodeBlock, EngineGlyph, KeyValue } from "@tandem/ui";
import { t } from "../../i18n";
import { riskLabel, riskVariant } from "./model";

/** The fields of a plugin ApprovalCard (F-PL-07, UI-93): plugin, account,
 * action, target, cost and risk class, with the full request (secrets
 * already masked by the gateway) behind a toggle. */
export function PluginApprovalFields({ detail }: { detail: PluginApprovalDetail }) {
  const [open, setOpen] = useState(false);
  const spend = detail.riskClass === "spend";
  return (
    <div className="flex flex-col gap-2" data-testid="plugin-approval" data-risk={detail.riskClass}>
      <div className="flex items-center gap-2">
        <EngineGlyph letters={detail.logo} size={16} />
        <span className="text-sm font-medium text-fg">{detail.pluginLabel}</span>
        <Badge variant={riskVariant(detail.riskClass)}>{riskLabel(detail.riskClass)}</Badge>
      </div>
      <KeyValue
        items={[
          { key: t("plugins.approval.action"), value: detail.action },
          {
            key: t("plugins.approval.account"),
            value: detail.accountLabel ?? t("plugins.detail.unknownAccount"),
          },
          ...(detail.target ? [{ key: t("plugins.approval.target"), value: detail.target }] : []),
          {
            key: t("plugins.approval.cost"),
            value: (
              <span className={spend ? "text-error" : undefined} data-testid="plugin-approval-cost">
                {detail.cost ?? t("plugins.approval.noCost")}
              </span>
            ),
          },
        ]}
      />
      <Button
        size="sm"
        variant="ghost"
        className="self-start"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        {open ? t("plugins.approval.hideRequest") : t("plugins.approval.fullRequest")}
      </Button>
      {open && (
        <div data-testid="plugin-approval-request">
          <CodeBlock code={detail.request} copyLabel={t("thread.copy")} copiedLabel={t("thread.copied")} />
        </div>
      )}
    </div>
  );
}

/** The card's words for a plugin approval: "Approve", and "Allow for the
 * rest of this chat" where the gateway offers it (write and publish). */
export function pluginApprovalLabels() {
  return {
    allow: t("plugins.approval.approve"),
    allowThread: t("plugins.approval.allowChat"),
  };
}
