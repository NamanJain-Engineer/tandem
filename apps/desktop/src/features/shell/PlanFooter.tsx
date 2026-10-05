import { cx } from "@tandem/ui";
import { t } from "../../i18n";
import { totalCredits, useSession } from "../../stores/session";
import { useUi } from "../../stores/ui";

/** Rail footer: "FREE", or "PRO · 10,000 credits" (F-SH-03 slice). */
export function PlanFooter({ collapsed }: { collapsed?: boolean }) {
  const auth = useSession((s) => s.auth);
  const e = useSession((s) => s.entitlement);
  const openSettings = useUi((s) => s.openSettings);
  const credits = totalCredits(e) ?? 0;
  const text = !auth?.signedIn
    ? t("plan.footer.offline")
    : !e || e.plan === "free"
      ? t("plan.footer.free")
      : t("plan.footer.paid", { plan: e.plan.toUpperCase(), n: credits });
  return (
    <div className="border-t border-divider p-1.5">
      <button
        type="button"
        data-testid="plan-footer"
        onClick={() => openSettings("account")}
        className={cx(
          "flex h-7 w-full items-center rounded-sm px-2 text-2xs font-semibold tracking-caps text-fg-muted hover:bg-surface-4 focus-visible:shadow-focus focus-visible:outline-none",
          collapsed && "justify-center px-0",
        )}
      >
        <span className="truncate">{collapsed ? text.split(" ")[0] : text}</span>
      </button>
    </div>
  );
}
