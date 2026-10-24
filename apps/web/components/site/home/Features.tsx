import { SCENE_ROWS } from "../../../lib/home";
import { SCENE_BUILD, atLeast } from "../../../lib/scenes";
import { Scene } from "../scenes/Scene";
import { DASHBOARD_CAPTION, DashboardScene, dashboardSceneText } from "../scenes/DashboardScene";
import { LAND_CAPTION, LandScene, landSceneText } from "../scenes/LandScene";
import { REVIEW_CAPTION, ReviewScene, reviewSceneText } from "../scenes/ReviewScene";
import { Section } from "../Section";
import { TextLink } from "../TextLink";

const SCENES = {
  dashboard: { scene: <DashboardScene />, caption: DASHBOARD_CAPTION, text: dashboardSceneText },
  review: { scene: <ReviewScene />, caption: REVIEW_CAPTION, text: reviewSceneText },
  land: { scene: <LandScene />, caption: LAND_CAPTION, text: landSceneText },
} as const;

/** #features (DESIGN §5.1, D-1881): what it's like, as scenes of the app
 * instead of paragraphs. One short claim per row, its scene below it at the
 * container's full width, so the app's text reads at its own size.
 * A row whose release isn't out yet isn't shown (D-128). */
export function Features() {
  const rows = SCENE_ROWS.filter((r) => atLeast(SCENE_BUILD, r.release));
  return (
    <Section id="features" eyebrow="Working today" align="center" title="Many agents, one calm view">
      <div className="flex flex-col gap-24 lg:gap-32">
        {rows.map((r) => {
          const s = SCENES[r.id];
          return (
            <div key={r.id} className="flex flex-col gap-10">
              <div className="grid gap-5 lg:grid-cols-12 lg:items-end lg:gap-6">
                <div className="flex flex-col gap-5 lg:col-span-6">
                  <p className="reveal eyebrow">{r.eyebrow}</p>
                  <h3 className="reveal stagger-1 text-display-md text-balance text-fg">{r.title}</h3>
                </div>
                <div className="flex flex-col gap-4 lg:col-span-5 lg:col-start-8">
                  <p className="reveal stagger-2 text-copy text-pretty text-fg-muted">{r.body}</p>
                  <TextLink href={r.docs} className="reveal stagger-3 self-start">
                    How it works
                  </TextLink>
                </div>
              </div>
              <Scene id={r.id} text={s.text()} caption={s.caption} className="rise">
                {s.scene}
              </Scene>
            </div>
          );
        })}
      </div>
    </Section>
  );
}
