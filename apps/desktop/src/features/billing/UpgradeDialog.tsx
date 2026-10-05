import { useEffect, useRef, useState } from "react";
import { Check, ExternalLink } from "lucide-react";
import { Badge, Button, Dialog, SegmentedControl, Spinner, cx, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { invoke } from "../../ipc/invoke";
import { useSession } from "../../stores/session";
import { useUi } from "../../stores/ui";
import { openBilling } from "../shell/actions";

type Stage = "choose" | "waiting" | "success" | "timeout";
const POLL_MS = 5_000;
const MAX_WAIT_MS = 15 * 60_000;

/** UI-120 plans and UI-121 checkout handoff (F-BL-01, F-BL-04). */
export function UpgradeDialog() {
  const { open, gate, retry } = useUi((s) => s.upgrade);
  const close = useUi((s) => s.closeUpgrade);
  const auth = useSession((s) => s.auth);
  const plan = useSession((s) => s.entitlement?.plan ?? "free");
  const refresh = useSession((s) => s.refreshEntitlement);
  const { show } = useToast();
  const [interval, setInterval_] = useState<"month" | "year">("month");
  const [stage, setStage] = useState<Stage>("choose");
  const [lastTarget, setLastTarget] = useState<{ plan: string; interval: string } | null>(null);
  const startedAt = useRef(0);

  useEffect(() => {
    if (open) {
      setStage(plan === "free" ? "choose" : "choose");
      invoke("telemetry_capture", {
        event: "upgrade_sheet_opened",
        props: { source: gate ? "gate" : "account_menu" },
      }).catch(() => undefined);
    }
  }, [open, gate, plan]);

  // The plan arrives over /me/events within seconds; poll as a backstop.
  useEffect(() => {
    if (stage !== "waiting") return;
    if (plan !== "free") {
      setStage("success");
      return;
    }
    const timer = window.setInterval(() => {
      if (Date.now() - startedAt.current > MAX_WAIT_MS) setStage("timeout");
      else void refresh();
    }, POLL_MS);
    return () => window.clearInterval(timer);
  }, [stage, plan, refresh]);

  const checkout = async (target: { plan: string; interval: string }) => {
    if (!auth?.signedIn) {
      invoke("auth_sign_in").catch(() => undefined);
      close();
      return;
    }
    setLastTarget(target);
    if (await openBilling({ type: "upgrade", ...target }, show)) {
      startedAt.current = Date.now();
      setStage("waiting");
    }
  };

  const signedOut = !auth?.signedIn;
  const card = "flex flex-1 flex-col gap-3 rounded-md border border-border bg-surface-2 p-4";
  const feature = (text: string) => (
    <li key={text} className="flex items-start gap-1.5 text-sm text-fg-muted">
      <Check size={14} aria-hidden="true" className="mt-0.5 shrink-0 text-accent" />
      {text}
    </li>
  );

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => !o && close()}
      size="lg"
      title={stage === "success" ? t("checkout.success") : t("upgrade.title")}
      closeLabel={t("common.close")}
    >
      {stage === "choose" && (
        <div className="flex flex-col gap-4">
          {gate && (
            <p className="text-base text-fg" data-testid="upgrade-gate">
              {gate.kind === "workspaces"
                ? t("upgrade.gate.workspaces", { n: gate.limit })
                : t("upgrade.gate.panes", { n: gate.limit })}
            </p>
          )}
          <div className="flex justify-center">
            <SegmentedControl
              label={t("upgrade.interval")}
              size="sm"
              value={interval}
              onValueChange={(v) => setInterval_(v as "month" | "year")}
              items={[
                { id: "month", label: t("upgrade.monthly") },
                { id: "year", label: t("upgrade.yearly") },
              ]}
            />
          </div>
          <div className="flex gap-3">
            <section aria-label={t("plan.free")} className={card}>
              <h3 className="text-md font-semibold text-fg">{t("plan.free")}</h3>
              <p className="text-lg font-semibold text-fg">{t("upgrade.free.price")}</p>
              <ul
                className={cx(
                  "flex flex-1 flex-col gap-1",
                  gate?.kind === "workspaces" &&
                    "[&>li:nth-child(2)]:rounded-xs [&>li:nth-child(2)]:bg-accent-soft",
                )}
              >
                {[t("upgrade.free.f1"), t("upgrade.free.f2"), t("upgrade.free.f3"), t("upgrade.free.f4")].map(
                  feature,
                )}
              </ul>
              <Button disabled={plan === "free"} fullWidth>
                {t("upgrade.current")}
              </Button>
            </section>
            <section aria-label={t("plan.pro")} className={cx(card, "border-accent")}>
              <div className="flex items-center gap-2">
                <h3 className="text-md font-semibold text-fg">{t("plan.pro")}</h3>
                <Badge variant="accent">{t("upgrade.recommended")}</Badge>
              </div>
              <p className="text-lg font-semibold text-fg">
                {interval === "month" ? t("upgrade.pro.price.month") : t("upgrade.pro.price.year")}
              </p>
              <ul className="flex flex-1 flex-col gap-1">
                {[t("upgrade.pro.f1"), t("upgrade.pro.f2"), t("upgrade.pro.f3"), t("upgrade.pro.f4")].map(
                  feature,
                )}
              </ul>
              <Button
                variant="primary"
                fullWidth
                data-testid="upgrade-pro"
                disabled={plan === "pro"}
                onClick={() => void checkout({ plan: "pro", interval })}
              >
                {signedOut ? t("upgrade.signInToUpgrade") : t("upgrade.proTrial")}
              </Button>
              <p className="text-2xs text-fg-subtle">{t("upgrade.finePrint")}</p>
            </section>
            <section aria-label={t("plan.team")} className={card}>
              <h3 className="text-md font-semibold text-fg">{t("plan.team")}</h3>
              <p className="text-lg font-semibold text-fg">
                {interval === "month" ? t("upgrade.team.price.month") : t("upgrade.team.price.year")}
              </p>
              <ul className="flex flex-1 flex-col gap-1">
                {[
                  t("upgrade.team.f1"),
                  t("upgrade.team.f2"),
                  t("upgrade.team.f3"),
                  t("upgrade.team.f4"),
                  t("upgrade.team.f5"),
                ].map(feature)}
              </ul>
              <Button
                fullWidth
                disabled={plan === "team"}
                onClick={() => void checkout({ plan: "team", interval })}
              >
                {signedOut ? t("upgrade.signInToUpgrade") : t("upgrade.teamTrial")}
              </Button>
            </section>
          </div>
          <div className="flex items-start gap-4 text-sm text-fg-subtle">
            <p className="flex-1">{t("upgrade.footer")}</p>
            <Button
              variant="link"
              size="sm"
              iconRight={<ExternalLink size={12} />}
              onClick={() =>
                invoke("app_open_external", { url: "https://tandem.dev/pricing" }).catch(() => undefined)
              }
            >
              {t("upgrade.compare")}
            </Button>
          </div>
        </div>
      )}
      {stage === "waiting" && (
        <div role="status" className="flex flex-col gap-4">
          <p className="inline-flex items-center gap-2 text-md text-fg">
            {t("checkout.waiting")}
            <Spinner />
          </p>
          <p className="text-base text-fg-muted">{t("checkout.waitingBody")}</p>
          <div className="flex gap-2">
            <Button onClick={() => lastTarget && void openBilling({ type: "upgrade", ...lastTarget }, show)}>
              {t("checkout.openAgain")}
            </Button>
            <Button variant="primary" onClick={() => void refresh()}>
              {t("checkout.finished")}
            </Button>
            <Button variant="ghost" onClick={() => setStage("choose")}>
              {t("common.cancel")}
            </Button>
          </div>
        </div>
      )}
      {stage === "timeout" && (
        <div role="alert" className="flex items-center gap-3">
          <p className="flex-1 text-base text-fg">{t("checkout.notConfirmed")}</p>
          <Button onClick={() => void refresh().then((e) => e && e.plan !== "free" && setStage("success"))}>
            {t("checkout.refresh")}
          </Button>
        </div>
      )}
      {stage === "success" && (
        <div className="flex flex-col gap-4">
          <ul className="flex flex-col gap-1.5">
            {[t("checkout.unlocked1"), t("checkout.unlocked2"), t("checkout.unlocked3")].map(feature)}
          </ul>
          <div className="flex justify-end gap-2">
            {retry && (
              <Button
                onClick={() => {
                  close();
                  retry();
                }}
              >
                {t("checkout.retryBlocked")}
              </Button>
            )}
            <Button variant="primary" onClick={close}>
              {t("checkout.done")}
            </Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
