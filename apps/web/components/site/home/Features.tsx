import type { CSSProperties } from "react";
import { SCENE_ROWS } from "../../../lib/home";
import { SCENE_BUILD, atLeast } from "../../../lib/scenes";
import { Scene } from "../scenes/Scene";
import { DASHBOARD_CAPTION, DashboardScene, dashboardSceneText } from "../scenes/DashboardScene";
import { REVIEW_CAPTION, ReviewScene, reviewSceneText } from "../scenes/ReviewScene";
import { TextLink } from "../TextLink";
import "./story.css";

const SCENES = {
  dashboard: { scene: <DashboardScene />, caption: DASHBOARD_CAPTION, text: dashboardSceneText },
  review: { scene: <ReviewScene />, caption: REVIEW_CAPTION, text: reviewSceneText },
} as const;

/** The scroll timeline a chapter's first step names, so the frame can hand
 * over to that chapter's scene as the step comes up (story.css). */
const chapter = (i: number) => `--story-${i}`;

/** #features (DESIGN §5.1, D-1881): what it's like, told as a story the
 * visitor scrolls through. On a wide screen with motion allowed the product
 * frame stays pinned beside the steps; the step in front lights up and the
 * frame cross-fades to its chapter's scene, all by scroll timelines in
 * story.css, with no script (the exported page keeps none of its own). On a
 * phone, under reduced motion or without scroll timelines it's a plain
 * stack: each chapter's steps, then its scene. A row whose release isn't
 * out yet isn't shown (D-128). */
export function Features() {
  const rows = SCENE_ROWS.filter((r) => atLeast(SCENE_BUILD, r.release));
  const count = rows.reduce((n, r) => n + r.steps.length, 0);
  const scope = rows.map((_, i) => chapter(i)).join(", ");
  /** The number of the step before each chapter's first. */
  const before = rows.map((_, i) => rows.slice(0, i).reduce((n, r) => n + r.steps.length, 0));
  return (
    <section
      id="features"
      aria-labelledby="features-title"
      className="relative scroll-mt-section bg-page py-(--site-section-y)"
    >
      <div className="container-site">
        <div className="story" style={{ "--story-steps": count, "--story-scope": scope } as CSSProperties}>
          <header className="story-head flex flex-col gap-5">
            <p className="reveal eyebrow">Working today</p>
            <h2
              id="features-title"
              className="reveal stagger-1 text-sheen max-w-[20em] text-display-lg text-fg"
            >
              Many agents, one screen
            </h2>
            <p className="reveal stagger-2 max-w-measure text-lead text-pretty text-fg-muted">
              Each agent runs in a real terminal over your own folder. Split, arrange and keep going.
            </p>
            {rows[0] && (
              <TextLink href={rows[0].docs} className="reveal stagger-3 self-start">
                How it works
              </TextLink>
            )}
          </header>
          {rows.map((r, ci) => {
            const s = SCENES[r.id];
            const layer = {
              ...(ci > 0 && { "--story-in": chapter(ci) }),
              ...(ci < rows.length - 1 && { "--story-out": chapter(ci + 1) }),
            } as CSSProperties;
            return (
              <div key={r.id} className="story-chapter">
                {r.steps.map((step, si) => {
                  const n = (before[ci] ?? 0) + si + 1;
                  return (
                    <div
                      key={step.title}
                      className="story-step"
                      style={si === 0 ? ({ "--story-name": chapter(ci) } as CSSProperties) : undefined}
                    >
                      <div className="story-body flex flex-col gap-3">
                        <span aria-hidden="true" className="story-tick" />
                        <p className="flex items-center gap-3 font-mono text-eyebrow uppercase tracking-[0.12em] text-fg-subtle">
                          <span aria-hidden="true" className="story-num tabular-nums">
                            {String(n).padStart(2, "0")}
                          </span>
                          {r.eyebrow}
                        </p>
                        <h3 className="text-display-md text-balance text-fg">{step.title}</h3>
                        <p className="text-copy text-pretty text-fg-muted">{step.body}</p>
                      </div>
                    </div>
                  );
                })}
                <div
                  className="story-layer"
                  data-in={ci > 0 ? "" : undefined}
                  data-out={ci < rows.length - 1 ? "" : undefined}
                  style={layer}
                >
                  <Scene id={r.id} text={s.text()} caption={s.caption} className="rise">
                    {s.scene}
                  </Scene>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </section>
  );
}
