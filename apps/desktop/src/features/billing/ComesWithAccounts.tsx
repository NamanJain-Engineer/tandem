import { Button } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";

/** Where `Tell me when accounts open` writes to (DESIGN §9.5). */
export const NOTIFY_HREF = `mailto:contact@vibezeo.com?subject=${encodeURIComponent(
  "Tell me when Vibezeo accounts open",
)}`;

/** A hosted feature's place when it can't run: it needs Vibezeo's servers.
 * The words are DESIGN §9.5's, word for word: never a plan, a price or
 * credits (D-869, D-2903). */
export function ComesWithAccounts({ testId, action = true }: { testId?: string; action?: boolean }) {
  return (
    <span className="flex flex-col items-start gap-0.5 text-sm" data-testid={testId ?? "comes-with-accounts"}>
      <span className="font-medium text-fg">{t("hosted.withAccounts")}</span>
      <span className="text-fg-muted">{t("hosted.why")}</span>
      {action && (
        <Button
          variant="link"
          size="sm"
          onClick={() => invoke("app_open_external", { url: NOTIFY_HREF }).catch(() => undefined)}
        >
          {t("hosted.notify")}
        </Button>
      )}
    </span>
  );
}
