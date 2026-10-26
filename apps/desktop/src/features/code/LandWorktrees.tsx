import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  LandingCandidate,
  LandingPlan,
  LandingRun,
  LandingStep,
  WorkspaceSummary,
} from "@tandem/protocol";
import { Banner, Button, Checkbox, CodeBlock, Dialog, Input, Skeleton, Spinner, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import {
  LANDING_POLL_MS,
  landingPlan,
  landingRun,
  startLanding,
  stopLanding,
  undoLanding,
} from "../../stores/worktrees";
import { SendToAgent } from "./SendToAgent";

type Load = { at: "loading" } | { at: "error"; message: string } | { at: "ready"; plan: LandingPlan };

const landable = (c: LandingCandidate) => c.uncommitted === 0 && !c.missing;

function forecastText(c: LandingCandidate): string {
  if (c.missing) return t("landing.missing");
  if (c.uncommitted > 0) return t("landing.uncommitted");
  const commits = t("landing.commits", { n: c.ahead });
  const f = c.forecast;
  const forecast =
    f.kind === "clean"
      ? t("landing.forecast.clean")
      : f.kind === "conflicts"
        ? t("landing.forecast.conflicts", { n: f.files.length })
        : t("landing.forecast.none");
  return `${commits} · ${forecast}`;
}

function stepText(s: LandingStep, base: string): string {
  switch (s.state) {
    case "waiting":
      return t("landing.step.waiting");
    case "merging_base":
      return t("landing.step.mergingBase", { base });
    case "checking":
      return t("landing.step.checking");
    case "landing":
      return t("landing.step.landing");
    case "landed":
      return s.commit
        ? t("landing.step.landed", { commit: s.commit.slice(0, 7) })
        : t("landing.step.already", { base });
    case "failed":
      return t("landing.step.failed", { code: s.exit ?? "—" });
    case "conflict":
      return t("landing.step.conflict", { files: s.conflicts.join(", ") });
    case "stopped":
      return t("landing.step.stopped");
  }
}

function Steps({ run }: { run: LandingRun }) {
  return (
    <ol className="flex flex-col gap-2" data-testid="landing-steps" data-state={run.state}>
      {run.steps.map((s) => (
        <li
          key={s.laneId}
          data-testid="landing-step"
          data-branch={s.branch}
          data-state={s.state}
          className="flex flex-col gap-1 rounded-sm border border-border px-2 py-1.5"
        >
          <div className="flex items-center gap-2 text-sm">
            {["merging_base", "checking", "landing"].includes(s.state) && <Spinner size={12} />}
            <span className="truncate font-mono text-fg">{s.branch}</span>
            <span className="ml-auto text-fg-muted">{stepText(s, run.base)}</span>
          </div>
          {s.output && (s.state === "failed" || s.state === "stopped") ? (
            <CodeBlock code={s.output} language="text" />
          ) : null}
          {s.state === "failed" && s.prompt ? (
            <SendToAgent laneId={s.laneId} prompt={s.prompt} testId="landing" />
          ) : null}
        </li>
      ))}
    </ol>
  );
}

/** UI-155 "Land worktrees" (F-WT-07): merge a repository's worktrees into
 * its current branch one at a time, each after the base and the check. */
/** The candidates with `first` (a plan's worktrees, F-TK-07) leading in
 * that order, then the rest as the plan lists them. */
export function plannedOrder(candidates: LandingCandidate[], first?: string[]): LandingCandidate[] {
  if (!first?.length) return candidates;
  const at = (c: LandingCandidate) => {
    const i = first.indexOf(c.laneId);
    return i < 0 ? first.length : i;
  };
  return [...candidates].sort((a, b) => at(a) - at(b));
}

export function LandWorktreesDialog({
  ws,
  laneIds,
  onClose,
}: {
  ws: WorkspaceSummary;
  /** Opened from a plan (F-TK-07): only these are chosen, in this order. */
  laneIds?: string[];
  onClose: () => void;
}) {
  const [load, setLoad] = useState<Load>({ at: "loading" });
  const [run, setRun] = useState<LandingRun | null>(null);
  const [chosen, setChosen] = useState<string[]>([]);
  const [check, setCheck] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { show } = useToast();

  const readPlan = useCallback(async () => {
    const out = await landingPlan(ws.id);
    if (!out.ok) {
      setLoad({ at: "error", message: out.error.message });
      return null;
    }
    setLoad({ at: "ready", plan: out.value });
    return out.value;
  }, [ws.id]);

  const wanted = laneIds?.join("\n");
  const fetchAll = useCallback(() => {
    setLoad({ at: "loading" });
    const only = wanted?.split("\n");
    void Promise.all([readPlan(), landingRun(ws.id)]).then(([plan, current]) => {
      if (plan) {
        setChosen(
          plan.candidates
            .filter(landable)
            .filter((c) => !only || only.includes(c.laneId))
            .map((c) => c.laneId),
        );
        setCheck(plan.check ?? "");
      }
      if (current.ok) setRun(current.value);
    });
  }, [readPlan, ws.id, wanted]);
  useEffect(() => fetchAll(), [fetchAll]);

  // A running landing is read every second; once it ends, the plan again
  // (what Undo landing would undo).
  const running = run?.state === "running";
  useEffect(() => {
    if (!running) return;
    const timer = setInterval(() => {
      void landingRun(ws.id).then((out) => {
        if (out.ok && out.value) setRun(out.value);
      });
    }, LANDING_POLL_MS);
    return () => clearInterval(timer);
  }, [running, ws.id]);
  useEffect(() => {
    if (run && run.state !== "running") void readPlan();
  }, [run, readPlan]);

  const plan = load.at === "ready" ? load.plan : null;
  const base = plan?.base ?? "";
  const candidates = useMemo(() => plannedOrder(plan?.candidates ?? [], wanted?.split("\n")), [plan, wanted]);
  const order = candidates.map((c) => c.laneId).filter((id) => chosen.includes(id));
  const toggle = (id: string, on: boolean) => setChosen((c) => (on ? [...c, id] : c.filter((x) => x !== id)));

  const land = async () => {
    setBusy(true);
    setError(null);
    const out = await startLanding(ws.id, order, check.trim() || null);
    setBusy(false);
    if (out.ok) setRun(out.value);
    else setError(out.error.message);
  };
  const undo = async () => {
    setBusy(true);
    setError(null);
    const out = await undoLanding(ws.id);
    setBusy(false);
    if (out.ok) {
      show({ tone: "success", title: t("landing.undone", { base: plan?.undo?.base ?? base }) });
      setRun(null);
      fetchAll();
    } else if (out.error.code !== "cancelled") setError(out.error.message);
  };

  const canLand = plan !== null && plan.base !== null && !plan.baseDirty && order.length > 0 && !busy;
  const footer = (
    <>
      {plan?.undo && !running ? (
        <Button variant="danger" onClick={() => void undo()} disabled={busy} data-testid="landing-undo">
          {t("landing.undo")}
        </Button>
      ) : null}
      <span className="flex-1" />
      {running ? (
        <Button variant="secondary" onClick={() => void stopLanding(ws.id)} data-testid="landing-stop">
          {t("landing.stop")}
        </Button>
      ) : run ? (
        <>
          <Button variant="secondary" onClick={() => setRun(null)} data-testid="landing-again">
            {t("landing.again")}
          </Button>
          <Button variant="primary" onClick={onClose}>
            {t("common.close")}
          </Button>
        </>
      ) : (
        <>
          <Button variant="secondary" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={!canLand}
            onClick={() => void land()}
            data-testid="landing-land"
          >
            {t("landing.land", { n: order.length })}
          </Button>
        </>
      )}
    </>
  );

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && !running && onClose()}
      title={t("landing.title")}
      description={t("landing.description", { base: run?.base ?? (base || "—") })}
      closeLabel={t("common.close")}
      size="lg"
      footer={footer}
    >
      <div data-testid="landing" className="flex flex-col gap-3">
        {load.at === "loading" && !run ? (
          <div className="flex flex-col gap-2" aria-busy="true">
            <Skeleton height={32} />
            <Skeleton height={32} />
            <Skeleton height={32} />
          </div>
        ) : load.at === "error" && !run ? (
          <Banner tone="error" action={{ label: t("worktree.setup.retry"), onClick: fetchAll }}>
            {load.message}
          </Banner>
        ) : run ? (
          <>
            {run.error ? <Banner tone="error">{run.error}</Banner> : null}
            {run.state === "done" ? (
              <Banner tone="success">{t("landing.done", { n: run.steps.length, base: run.base })}</Banner>
            ) : null}
            <Steps run={run} />
          </>
        ) : plan ? (
          <>
            {plan.interrupted ? (
              <Banner tone="warning">
                <span data-testid="landing-interrupted">
                  {t("landing.interrupted", { folder: plan.interrupted })}
                </span>
              </Banner>
            ) : null}
            {plan.base === null ? (
              <Banner tone="error">{t("landing.detached", { folder: ws.name })}</Banner>
            ) : plan.baseDirty ? (
              <Banner tone="warning">{t("landing.dirty", { folder: ws.name })}</Banner>
            ) : null}
            {plan.candidates.length === 0 ? (
              <p data-testid="landing-empty" className="text-sm text-fg-subtle">
                {t("landing.empty", { base: base || "HEAD" })}
              </p>
            ) : (
              <div className="flex flex-col gap-1">
                {candidates.map((c) => (
                  <div
                    key={c.laneId}
                    data-testid="landing-candidate"
                    data-branch={c.branch}
                    data-lane={c.laneId}
                  >
                    <Checkbox
                      label={c.branch}
                      description={forecastText(c)}
                      checked={chosen.includes(c.laneId)}
                      disabled={!landable(c) || busy}
                      onCheckedChange={(on) => toggle(c.laneId, on)}
                    />
                  </div>
                ))}
              </div>
            )}
            <Input
              label={t("landing.checkLabel")}
              hint={check.trim() ? t("landing.checkHint") : t("landing.noCheck")}
              monospace
              maxLength={4000}
              value={check}
              onChange={(e) => setCheck(e.target.value)}
              data-testid="landing-check"
            />
          </>
        ) : null}
        {error ? <Banner tone="error">{error}</Banner> : null}
      </div>
    </Dialog>
  );
}
