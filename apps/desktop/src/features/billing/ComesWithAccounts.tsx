import { t } from "../../i18n";

/** Where a hosted feature would be switched on: it needs Vibezeo's servers
 * and comes with Vibezeo accounts, never a plan or a price (D-869, D-2903). */
export function ComesWithAccounts({ testId }: { testId?: string }) {
  return (
    <span className="text-sm text-fg-muted" data-testid={testId ?? "comes-with-accounts"}>
      {t("hosted.withAccounts")}
    </span>
  );
}
