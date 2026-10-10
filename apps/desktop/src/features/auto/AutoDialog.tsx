import { useEffect, useState } from "react";
import type { AutoMode } from "@tandem/protocol";
import { Button, Checkbox, Dialog, Input, RadioGroup, Textarea } from "@tandem/ui";
import { t } from "../../i18n";
import { CEILINGS, MODES, startAuto, useAuto } from "../../stores/auto";
import { useSettings } from "../../stores/settings";

type Step = "mode" | "consent";

/** Pane ⋯ → Auto… (F-AU-01): choose a mode, then (until "Don't ask again")
 * agree that the terminal's last lines leave the machine. */
export function AutoDialog() {
  const paneId = useAuto((s) => s.dialogFor);
  const consent = useSettings((s) => s.values.auto_consent === true);
  const dontAsk = useSettings((s) => s.values.auto_dont_ask === true);
  const [step, setStep] = useState<Step>("mode");
  const [mode, setMode] = useState<AutoMode>("finish");
  const [goal, setGoal] = useState("");
  const [ceiling, setCeiling] = useState(String(CEILINGS.custom));
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (paneId) {
      setStep("mode");
      setError(null);
      setBusy(false);
      setRemember(false);
    }
  }, [paneId]);

  const close = () => useAuto.getState().closeDialog();
  const turns = Number(ceiling);
  const customOk = goal.trim().length > 0 && Number.isInteger(turns) && turns >= 1 && turns <= 30;
  const ready = mode !== "custom" || customOk;

  const start = async () => {
    if (!paneId) return;
    setBusy(true);
    setError(null);
    const out = await startAuto(
      paneId,
      mode,
      mode === "custom" ? goal.trim() : null,
      mode === "custom" ? turns : null,
    );
    setBusy(false);
    if (out.ok) close();
    else {
      setStep("mode");
      setError(out.error.message);
    }
  };

  const next = async () => {
    if (consent && dontAsk) return start();
    setStep("consent");
  };

  const agree = async () => {
    const settings = useSettings.getState();
    if (!(await settings.set("auto_consent", true))) {
      setError(t("auto.consentFailed"));
      setStep("mode");
      return;
    }
    if (remember) await settings.set("auto_dont_ask", true);
    await start();
  };

  const items = MODES.map((m) => ({
    value: m,
    label: t(`auto.mode.${m}`),
    description:
      m === "custom" ? t("auto.mode.custom.desc") : t(`auto.mode.${m}.desc`, { n: CEILINGS[m] }),
  }));

  return (
    <Dialog
      open={paneId !== null}
      onOpenChange={(o) => !o && close()}
      title={step === "mode" ? t("auto.title") : t("auto.consentTitle")}
      description={step === "mode" ? t("auto.description") : undefined}
      footer={
        step === "mode" ? (
          <>
            <Button variant="secondary" onClick={close}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" disabled={!ready} loading={busy} onClick={() => void next()} data-testid="auto-start">
              {t("auto.start")}
            </Button>
          </>
        ) : (
          <>
            <Button variant="secondary" onClick={() => setStep("mode")}>
              {t("auto.back")}
            </Button>
            <Button variant="primary" loading={busy} onClick={() => void agree()} data-testid="auto-consent-continue">
              {t("auto.consentContinue")}
            </Button>
          </>
        )
      }
    >
      {step === "mode" ? (
        <div className="flex flex-col gap-4" data-testid="auto-dialog">
          <RadioGroup label={t("auto.modeLabel")} items={items} value={mode} onValueChange={(v) => setMode(v as AutoMode)} />
          {mode === "custom" && (
            <div className="flex flex-col gap-3">
              <Textarea
                label={t("auto.goal")}
                value={goal}
                maxLength={2000}
                minRows={2}
                onChange={(e) => setGoal(e.target.value)}
                data-testid="auto-goal"
              />
              <Input
                label={t("auto.ceiling")}
                hint={t("auto.ceilingHint")}
                type="number"
                min={1}
                max={30}
                value={ceiling}
                onChange={(e) => setCeiling(e.target.value)}
                data-testid="auto-ceiling"
              />
            </div>
          )}
          {error && (
            <p role="alert" className="text-sm text-danger" data-testid="auto-error">
              {error}
            </p>
          )}
        </div>
      ) : (
        <div className="flex flex-col gap-3" data-testid="auto-consent">
          <p className="text-sm text-fg">{t("auto.consentBody")}</p>
          <Checkbox label={t("auto.dontAsk")} checked={remember} onCheckedChange={setRemember} />
        </div>
      )}
    </Dialog>
  );
}
