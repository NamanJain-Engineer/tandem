import { useState } from "react";
import { Banner, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useSession } from "../../stores/session";
import { openBilling } from "./actions";

type Shown = {
  id: string;
  tone: "info" | "warning" | "error";
  text: string;
  action?: { label: string; onClick: () => void };
  dismissable: boolean;
};

/** UI-133: one banner at a time — grace over > offline > offline-mode
 * deadline > payment failed. */
export function Banners() {
  const auth = useSession((s) => s.auth);
  const e = useSession((s) => s.entitlement);
  const refresh = useSession((s) => s.refreshEntitlement);
  const { show } = useToast();
  const [dismissed, setDismissed] = useState<Set<string>>(new Set());
  if (!auth) return null;

  let banner: Shown | null = null;
  if (auth.signedIn && e && !e.online && e.source === "free" && e.confirmedUntil !== null) {
    banner = {
      id: "grace",
      tone: "warning",
      text: t("banner.graceOver"),
      action: { label: t("banner.retry"), onClick: () => void refresh() },
      dismissable: false,
    };
  } else if (auth.signedIn && e && !e.online && e.confirmedUntil) {
    const time = new Date(Number(e.confirmedUntil)).toLocaleString("en-US", {
      weekday: "short",
      hour: "numeric",
      minute: "2-digit",
    });
    banner = { id: "offline", tone: "info", text: t("banner.offlineValid", { time }), dismissable: true };
  } else if (auth.offline && auth.offlineDaysLeft !== null && Number(auth.offlineDaysLeft) <= 3) {
    banner = {
      id: "offline-mode",
      tone: "warning",
      text: t("banner.offlineMode", { n: Number(auth.offlineDaysLeft) }),
      action: { label: t("banner.signIn"), onClick: () => invoke("auth_sign_in").catch(() => undefined) },
      dismissable: true,
    };
  } else if (auth.signedIn && e?.status === "past_due") {
    banner = {
      id: "past-due",
      tone: "error",
      text: t("banner.pastDueShort"),
      action: { label: t("banner.updatePayment"), onClick: () => void openBilling({ type: "portal" }, show) },
      dismissable: true,
    };
  }
  if (!banner || dismissed.has(banner.id)) return null;
  const id = banner.id;
  return (
    <Banner
      tone={banner.tone}
      action={banner.action}
      onDismiss={banner.dismissable ? () => setDismissed((d) => new Set(d).add(id)) : undefined}
      dismissLabel={t("common.dismiss")}
    >
      <span data-testid={`banner-${id}`}>{banner.text}</span>
    </Banner>
  );
}
