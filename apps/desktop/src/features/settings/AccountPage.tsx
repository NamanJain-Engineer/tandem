import { ExternalLink } from "lucide-react";
import { Badge, Button, KeyValue, SettingsGroup, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { totalCredits, useSession } from "../../stores/session";
import { useUi } from "../../stores/ui";
import { openBilling } from "../shell/actions";

const date = (iso: string | null | undefined) =>
  iso ? new Date(iso).toLocaleDateString("en-US", { day: "numeric", month: "short" }) : "";

/** UI-114 Account & Plan (F-BL-02 in-app). */
export function AccountPage() {
  const auth = useSession((s) => s.auth);
  const e = useSession((s) => s.entitlement);
  const refresh = useSession((s) => s.refreshEntitlement);
  const openUpgrade = useUi((s) => s.openUpgrade);
  const { show } = useToast();

  if (auth && !auth.hosted) {
    return (
      <SettingsGroup>
        <p className="p-3 text-base text-fg" data-testid="account-early-access">
          {t("settings.account.earlyAccess")}
        </p>
      </SettingsGroup>
    );
  }

  if (!auth?.signedIn) {
    return (
      <SettingsGroup>
        <div className="flex items-center gap-3 p-3">
          <p className="flex-1 text-base text-fg">
            {auth?.offline && auth.offlineDaysLeft !== null
              ? t("settings.account.offline", { n: Number(auth.offlineDaysLeft) })
              : t("settings.account.signedOut")}
          </p>
          <Button variant="primary" onClick={() => invoke("auth_sign_in").catch(() => undefined)}>
            {t("settings.account.signIn")}
          </Button>
        </div>
      </SettingsGroup>
    );
  }

  const plan = e?.plan ?? "free";
  const status = (e?.status ?? "none") as "active" | "trialing" | "past_due" | "canceled" | "none";
  const statusText =
    plan === "free"
      ? null
      : e?.cancelAtPeriodEnd && e.paidUntil
        ? t("settings.account.until", { date: date(e.paidUntil) })
        : status in { active: 1, trialing: 1, past_due: 1, canceled: 1 }
          ? `${t(`settings.account.status.${status}` as "settings.account.status.active")}${e?.paidUntil && status !== "past_due" ? ` · ${t("settings.account.renews", { date: date(e.paidUntil) })}` : ""}`
          : null;
  const total = totalCredits(e) ?? 0;
  const grant = e?.monthlyGrant ?? 10_000;

  return (
    <div className="flex flex-col gap-6">
      <SettingsGroup>
        <div className="flex items-center gap-3 p-3">
          <p className="flex-1 text-base text-fg">
            {t("settings.account.signedInAs", { email: auth.email ?? "" })}
          </p>
          <Button onClick={() => invoke("auth_sign_out").catch(() => undefined)}>
            {t("settings.account.signOut")}
          </Button>
        </div>
      </SettingsGroup>
      <SettingsGroup>
        <div className="flex flex-col gap-4 p-3">
          <KeyValue
            items={[
              {
                key: t("settings.account.plan"),
                value: (
                  <span className="inline-flex items-center gap-2" data-testid="account-plan">
                    <Badge variant={plan === "free" ? "neutral" : "accent"}>{plan.toUpperCase()}</Badge>
                    {statusText && <span className="text-fg-muted">{statusText}</span>}
                  </span>
                ),
              },
              ...(e?.credits
                ? [
                    {
                      key: t("settings.account.credits"),
                      value: (
                        <span className="flex flex-col">
                          <span className="tabular-nums">
                            {t("settings.account.creditsLine", { total, grant })}
                            {e.credits.resetsAt
                              ? ` · ${t("settings.account.resets", { date: date(e.credits.resetsAt) })}`
                              : ""}
                          </span>
                          <span className="text-sm text-fg-subtle tabular-nums">
                            {t("settings.account.creditsSplit", {
                              monthly: e.credits.monthly,
                              purchased: e.credits.purchased,
                            })}
                          </span>
                        </span>
                      ),
                    },
                  ]
                : []),
            ]}
          />
          <div className="flex flex-wrap gap-2">
            {plan === "free" ? (
              <Button variant="primary" onClick={() => openUpgrade()}>
                {t("settings.account.seePlans")}
              </Button>
            ) : (
              <Button
                iconRight={<ExternalLink size={14} />}
                onClick={() => void openBilling({ type: "portal" }, show)}
              >
                {t("settings.account.manageBilling")}
              </Button>
            )}
            <Button
              iconRight={<ExternalLink size={14} />}
              onClick={() => void openBilling({ type: "credits" }, show)}
            >
              {t("settings.account.buyCredits")}
            </Button>
            <Button variant="link" onClick={() => void refresh()}>
              {t("settings.account.refresh")}
            </Button>
          </div>
        </div>
      </SettingsGroup>
    </div>
  );
}
