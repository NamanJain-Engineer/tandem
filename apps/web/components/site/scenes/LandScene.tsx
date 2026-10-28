import type { ReactNode } from "react";
import { SAMPLE, t } from "../../../lib/scenes";
import { AppWindow, Changes, Dot, Glyph, StatusBar, fx } from "./parts";

const LANES = [
  { engine: "Claude Code", branch: SAMPLE.worktrees.claude.branch, commits: 3, commit: SAMPLE.commits[0] },
  { engine: "Codex", branch: SAMPLE.worktrees.codex.branch, commits: 5, commit: SAMPLE.commits[1] },
  { engine: "Gemini CLI", branch: SAMPLE.worktrees.gemini.branch, commits: 2, commit: SAMPLE.commits[2] },
] as const;

/** Each worktree's steps, 1.6 s apart and one worktree after another, as
 * the queue lands them (merge main in, run the check, land). */
const STEP = 0.55;
const LANE = 1.9;

function Steps({ lane, commit }: { lane: number; commit: string }) {
  const start = 0.8 + lane * LANE;
  const steps: ReadonlyArray<readonly [number, ReactNode]> = [
    [0, <><Dot status="idle" /> {t("landing.step.waiting")}</>],
    [start, <><Dot status="working" /> {t("landing.step.mergingBase", { base: SAMPLE.base })}</>],
    [start + STEP, <><Dot status="working" /> {t("landing.step.checking")}</>],
    [start + 2 * STEP, <><Dot status="working" /> {t("landing.step.landing")}</>],
    [start + 3 * STEP, <><Dot status="done" /> {t("landing.step.landed", { commit })}</>],
  ];
  return <Changes steps={steps} className="sc-step" />;
}

/** "Land them one by one" (D-1880, 0.9.0's F-WT-07): Land worktrees… takes
 * three agents' worktrees into main one at a time, merging main into each
 * and running the check first, then offers Undo landing. About 8 s, played
 * once. Shown only once the release has it (lib/scenes.ts `LANDS`). */
export function LandScene() {
  const done = 0.8 + LANES.length * LANE;
  return (
    <AppWindow className="sc-land">
      <div className="sc-land-body">
        <div className="sc-dialog" {...fx("rise", 0.2)}>
          <span className="sc-dash-title">{t("landing.title")}</span>
          <span className="sc-toast-body">{t("landing.description", { base: SAMPLE.base })}</span>
          <span className="sc-field">
            <span className="sc-field-label">{t("landing.checkLabel")}</span>
            <span className="sc-input">pnpm test</span>
          </span>
          <span className="sc-lanes">
            {LANES.map((l, i) => (
              <span key={l.branch} className="sc-lane-row" {...fx("in", 0.3 + i * 0.1)}>
                <span className="sc-tick" />
                <Glyph engine={l.engine} />
                <span className="sc-card-main">
                  <span className="sc-card-title sc-mono">{l.branch}</span>
                  <span className="sc-card-meta">
                    {t("landing.commits", { n: l.commits })} · {t("landing.forecast.clean")}
                  </span>
                </span>
                <Steps lane={i} commit={l.commit} />
              </span>
            ))}
          </span>
          <span className="sc-dialog-foot">
            <span className="sc-swap">
              <span {...fx("out", done)}>
                <span className="sc-btn sc-btn-primary" {...fx("press", 0.6)}>
                  {t("landing.land", { n: LANES.length })}
                </span>
              </span>
              <span className="sc-dialog-done" {...fx("pop", done)}>
                <Dot status="done" /> {t("landing.done", { n: LANES.length, base: SAMPLE.base })}
                <span className="sc-btn">{t("landing.undo")}</span>
              </span>
            </span>
          </span>
        </div>
      </div>
      <StatusBar branch={SAMPLE.base} />
    </AppWindow>
  );
}

export const LAND_CAPTION = "Three agents' worktrees land in main one at a time, each checked first, with undo.";

export function landSceneText(): string {
  return `Land worktrees takes three agents' worktrees into ${SAMPLE.base} one at a time. For each it merges ${SAMPLE.base} in, runs the check command, pnpm test, and lands it; then it reports all three landed and offers Undo landing.`;
}
