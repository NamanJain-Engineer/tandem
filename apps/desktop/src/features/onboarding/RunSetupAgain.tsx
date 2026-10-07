import { Button } from "@tandem/ui";
import { t } from "../../i18n";
import { useOnboarding } from "../../stores/onboarding";
import { useUi } from "../../stores/ui";

/** Settings › General → "Run setup again" (UI-101, F-SH-08). */
export function RunSetupAgain({ id }: { id: string }) {
  return (
    <Button
      id={id}
      size="sm"
      data-testid="run-setup-again"
      onClick={() => {
        useUi.getState().closeSettings();
        void useOnboarding.getState().restart();
      }}
    >
      {t("settings.runSetup")}
    </Button>
  );
}
