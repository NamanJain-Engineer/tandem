import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, PauseCircle } from "lucide-react";
import type { LimitView, PaneRecord } from "@tandem/protocol";
import {
  Button,
  Callout,
  ConfirmDialog,
  Dialog,
  RadioGroup,
  Select,
  useToast,
  type SelectOption,
} from "@tandem/ui";
import { t } from "../../i18n";
import { useAccounts } from "../../stores/accounts";
import { useEngines } from "../../stores/engines";
import {
  cancelResume,
  continueElsewhere,
  dismissLimit,
  resetClock,
  resumeAtReset,
  resumeNow,
  until,
  useLimits,
} from "../../stores/limits";

function useNow(every = 30_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), every);
    return () => clearInterval(id);
  }, [every]);
  return now;
}

function inText(at: number, now: number): string {
  const { h, m } = until(at, now);
  if (h === 0 && m === 0) return t("limits.untilSoon");
  return h > 0 ? t("limits.untilHm", { h, m }) : t("limits.untilM", { m });
}

/** "Work reached its limit · resets 4:12 PM (in 1h 48m)". */
export function limitTitle(limit: LimitView, account: string | null, now: number): string {
  const who = account ?? t("limits.thisAccount");
  return limit.resetsAt === null
    ? t("limits.reached", { account: who })
    : t("limits.reachedResets", {
        account: who,
        time: resetClock(limit.resetsAt),
        until: inText(limit.resetsAt, now),
      });
}

type Choice = "pause" | "other" | "stop";

/** UI-36 "Choose…": wait for the reset, continue on another account (with a
 * summary, after a confirm), or stop. Nothing moves until Continue. */
function LimitDialog({
  pane,
  limit,
  account,
  onClose,
}: {
  pane: PaneRecord;
  limit: LimitView;
  account: string | null;
  onClose: () => void;
}) {
  const accounts = useAccounts((s) => s.list);
  const engines = useEngines((s) => s.list);
  const now = useNow();
  const { show } = useToast();
  // The engine's other accounts first, then other installed engines.
  const targets: SelectOption[] = useMemo(() => {
    const same = accounts
      .filter((a) => a.engineId === pane.engineId && a.id !== limit.accountId)
      .map((a) => ({
        value: `${a.engineId}:${a.id}`,
        label: t("limits.targetAccount", {
          account: a.label,
          engine: engines.find((e) => e.spec.id === a.engineId)?.spec.label ?? a.engineId,
        }),
      }));
    const others = engines
      .filter((e) => e.install !== null && e.unsupportedReason === null && e.spec.id !== pane.engineId)
      .map((e) => ({ value: `${e.spec.id}:`, label: t("limits.targetEngine", { engine: e.spec.label }) }));
    return [...same, ...others];
  }, [accounts, engines, pane.engineId, limit.accountId]);
  const [choice, setChoice] = useState<Choice>(
    limit.resetsAt !== null ? "pause" : targets.length > 0 ? "other" : "stop",
  );
  const [target, setTarget] = useState(targets[0]?.value ?? "");
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);

  const moveOn = async () => {
    const [engineId, accountId] = target.split(":");
    setBusy(true);
    const out = await continueElsewhere(pane.id, engineId ?? "", accountId || null);
    setBusy(false);
    if (!out.ok) return show({ tone: "error", title: out.error.message });
    show({ tone: "success", title: t("limits.moved") });
    onClose();
  };
  const go = async () => {
    if (choice === "other") return setConfirm(true);
    const out = choice === "pause" ? await resumeAtReset(pane.id) : await dismissLimit(pane.id);
    if (!out.ok) show({ tone: "error", title: out.error.message });
    onClose();
  };

  return (
    <>
      <Dialog
        open={!confirm}
        size="md"
        onOpenChange={(o) => !o && onClose()}
        title={limitTitle(limit, account, now)}
        closeLabel={t("common.close")}
        footer={
          <>
            <Button variant="secondary" onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              loading={busy}
              disabled={choice === "other" && !target}
              onClick={() => void go()}
              data-testid="limit-go"
            >
              {t("limits.continue")}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-4" data-testid="limit-dialog">
          <p className="text-base text-fg-muted">{t("limits.body")}</p>
          <RadioGroup
            label={t("limits.how")}
            value={choice}
            onValueChange={(v) => setChoice(v as Choice)}
            items={[
              {
                value: "pause",
                label: t("limits.pause"),
                description:
                  limit.resetsAt !== null
                    ? t("limits.pauseHint", { time: resetClock(limit.resetsAt) })
                    : t("limits.noReset"),
                disabled: limit.resetsAt === null,
              },
              {
                value: "other",
                label: t("limits.other"),
                description: t("limits.otherHint"),
                disabled: targets.length === 0,
              },
              { value: "stop", label: t("limits.stop") },
            ]}
          />
          {choice === "other" && (
            <Select label={t("limits.target")} value={target} onValueChange={setTarget} options={targets} />
          )}
          {targets.length === 0 && <Callout tone="info">{t("limits.noOther")}</Callout>}
        </div>
      </Dialog>
      <ConfirmDialog
        open={confirm}
        onOpenChange={(o) => !o && setConfirm(false)}
        title={t("limits.summaryTitle")}
        body={t("limits.summaryBody")}
        confirmLabel={t("limits.continue")}
        onConfirm={() => void moveOn()}
      />
    </>
  );
}

/** UI-36 in a Code pane: a banner under the header with Resume at reset and
 * Choose…; while a resume is scheduled, "Paused · resumes 4:12 PM". */
export function LimitBanner({ pane }: { pane: PaneRecord }) {
  const limit = useLimits((s) => s.byPane[pane.id]);
  const account = useAccounts(
    (s) => s.list.find((a) => a.id === (limit?.accountId ?? pane.accountId))?.label ?? null,
  );
  const now = useNow();
  const [choosing, setChoosing] = useState(false);
  const { show } = useToast();
  if (!limit) return null;
  const act = async (run: () => Promise<{ ok: boolean; error?: { message: string } }>) => {
    const out = await run();
    if (!out.ok && out.error) show({ tone: "error", title: out.error.message });
  };
  const scheduled = limit.resumeAt !== null;
  return (
    <div
      data-testid="limit-banner"
      data-scheduled={scheduled ? "true" : "false"}
      role="status"
      className="flex min-h-8 shrink-0 flex-wrap items-center gap-2 border-b border-divider bg-waiting-soft px-2 py-1 text-sm text-fg"
    >
      {scheduled ? (
        <PauseCircle size={14} aria-hidden="true" className="shrink-0 text-warning-text" />
      ) : (
        <AlertTriangle size={14} aria-hidden="true" className="shrink-0 text-warning-text" />
      )}
      <span className="min-w-[220px] flex-1 truncate" title={limit.message}>
        {scheduled && limit.resumeAt !== null
          ? t("limits.paused", { time: resetClock(limit.resumeAt) })
          : limitTitle(limit, account, now)}
      </span>
      {scheduled ? (
        <>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void act(() => resumeNow(pane.id))}
            data-testid="limit-resume-now"
          >
            {t("limits.resumeNow")}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void act(() => cancelResume(pane.id))}
            data-testid="limit-cancel-resume"
          >
            {t("limits.cancelResume")}
          </Button>
        </>
      ) : (
        <>
          <Button
            size="sm"
            variant="ghost"
            disabled={limit.resetsAt === null}
            onClick={() => void act(() => resumeAtReset(pane.id))}
            data-testid="limit-resume-at-reset"
          >
            {t("limits.resumeAtReset")}
          </Button>
          <Button size="sm" variant="secondary" onClick={() => setChoosing(true)} data-testid="limit-choose">
            {t("limits.choose")}
          </Button>
        </>
      )}
      {choosing && (
        <LimitDialog pane={pane} limit={limit} account={account} onClose={() => setChoosing(false)} />
      )}
    </div>
  );
}
