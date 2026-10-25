import { Button, KeyValue, SettingsGroup } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { totalCredits, useSession } from "../../stores/session";
import { NOTIFY_HREF } from "../billing/ComesWithAccounts";

const date = (iso: string | null | undefined) =>
  iso ? new Date(iso).toLocaleDateString("en-US", { day: "numeric", month: "short" }) : "";

/** UI-114 Account: who's signed in, and the credits hosted features use.
 * Nothing is sold, so there's no plan, checkout or billing here (D-869,
 * D-2904). */
export function AccountPage() {
  const auth = useSession((s) => s.auth);
  const e = useSession((s) => s.entitlement);
  const refresh = useSession((s) => s.refreshEntitlement);

  if (auth && !auth.hosted) {
    return (
      <SettingsGroup>
        <div className="flex flex-col items-start gap-1 p-3">
          <p className="text-base text-fg" data-testid="account-early-access">
            {t("settings.account.earlyAccess")}
          </p>
          <Button
            variant="link"
            size="sm"
            onClick={() => invoke("app_open_external", { url: NOTIFY_HREF }).catch(() => undefined)}
          >
            {t("hosted.notify")}
          </Button>
        </div>
      </SettingsGroup>
    );
  }

  if (!auth?.signedIn) {
    return (
      <SettingsGroup>
        <div className="flex items-center gap-3 p-3">
          <p className="flex-1 text-base text-fg">
            {auth?.offline ? t("settings.account.offline") : t("settings.account.signedOut")}
          </p>
          <Button variant="primary" onClick={() => invoke("auth_sign_in").catch(() => undefined)}>
            {t("settings.account.signIn")}
          </Button>
        </div>
      </SettingsGroup>
    );
  }

  const total = totalCredits(e) ?? 0;
  const grant = e?.credits?.grant ?? e?.monthlyGrant ?? 10_000;
  const org = e?.org ?? null;

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
              ...(org
                ? [
                    {
                      key: t("settings.account.org"),
                      value: (
                        <span className="text-fg-muted" data-testid="account-org">
                          {t("settings.account.managedBy", { org: org.name })}
                        </span>
                      ),
                    },
                  ]
                : []),
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
                          {e.credits.cap !== null && (
                            <span className="text-sm text-fg-subtle tabular-nums" data-testid="account-cap">
                              {t("settings.account.cap", { spent: e.credits.spent ?? 0, cap: e.credits.cap })}
                            </span>
                          )}
                        </span>
                      ),
                    },
                  ]
                : []),
            ]}
          />
          <div className="flex flex-wrap gap-2">
            <Button variant="link" onClick={() => void refresh()}>
              {t("settings.account.refresh")}
            </Button>
          </div>
        </div>
      </SettingsGroup>
    </div>
  );
}
