import type { ReactNode } from "react";
import { SCENE_BUILD, atLeast } from "../../../lib/scenes";
import { CI_CAPTION, ChecksScene, ciSceneText } from "./ChecksScene";
import { LAND_CAPTION, LandScene, landSceneText } from "./LandScene";
import { PLAN_CAPTION, PlanScene, planSceneText } from "./PlanScene";

/** A product scene on a release's changelog page: what it shows, and the
 * Added card it sits directly above, full width (D-1898). */
export type ReleaseScene = {
  id: string;
  /** The Added card's bold title, without its full stop. */
  card: string;
  scene: () => ReactNode;
  caption: string;
  text: () => string;
  /** The scene's own stylesheet, beside scenes.css: only the pages that play
   * the scene inline it (D-3001). */
  css?: string;
};

/** The release pages' scenes, by version (D-1885, D-1888, D-3001). A page with
 * scenes loads the scene player, so scripts/script-allowlist.mjs lists the
 * same pages (a test checks). */
export const RELEASE_SCENES: Readonly<Record<string, readonly ReleaseScene[]>> = {
  "0.9.0": [
    { id: "land", card: "Land worktrees", scene: () => <LandScene />, caption: LAND_CAPTION, text: landSceneText },
    {
      id: "checks",
      card: "CI results and review comments back to the agent",
      scene: () => <ChecksScene />,
      caption: CI_CAPTION,
      text: ciSceneText,
      css: "ci.css",
    },
  ],
  "0.12.0": [{ id: "plan", card: "Follow a plan through", scene: () => <PlanScene />, caption: PLAN_CAPTION, text: planSceneText }],
};

/** `version`'s scenes, once the build the scenes show has that release
 * (D-128). */
export function releaseScenes(version: string): readonly ReleaseScene[] {
  return atLeast(SCENE_BUILD, version) ? (RELEASE_SCENES[version] ?? []) : [];
}
