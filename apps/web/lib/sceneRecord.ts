// What each bullet of a release's CHANGELOG section draws in the product
// scenes (F-WB-01 AC9, D-3006): the gate in lib/scenes.ts a scene draws it
// under, or "not drawn: " and why no scene does. A release's scenes slice
// adds its release's record, and scenes.test checks that the bullets of
// SCENES_SLICE's section (`## Unreleased` until the release renames it) are
// its keys, no more and no fewer, so SCENES_SLICE can't name a release with
// no slice behind it (the coordinator's S2 on 825b4b54).
import type * as Scenes from "./scenes";

/** The scenes' gates: each export of lib/scenes.ts that says whether the
 * release the scenes show has a change. */
export type SceneGate = { [K in keyof typeof Scenes]: (typeof Scenes)[K] extends boolean ? K : never }[keyof typeof Scenes];

/** A bullet's place in the scenes: the gate a scene draws it under, or why
 * no scene draws it. */
export type Drawn = SceneGate | `not drawn: ${string}`;

/** Each release's record, by version, then by each bullet's bold title as
 * CHANGELOG.md writes it. */
export const SCENE_RECORD: Readonly<Record<string, Readonly<Record<string, Drawn>>>> = {
  "0.14.0": {
    "The intent radar, live.":
      "not drawn: the hero's radar marks (RADAR, from 0.13.0) look the same whether the radar read the worktrees live or after a turn, and no scene draws Go or Java, a rename, or Claim these files",
    // The plan scene's third review goes back to its run and the plan ends
    // ready to land, so Landed rows and tasks marked Done aren't drawn.
    "Lead closes the loop.": "SENDS_BACK",
    "Keep and Run can't move under your click either.":
      "not drawn: no scene draws memory suggestions, Conductor, Execute all's check (the plan scene opens on the started plan), Edit & run, the pull-request checks' Allow (the CI scene's repository already reads its checks), a Vibezeo approval (the waiting agent asks in its own terminal, D-3001) or the phone app; the control the hero draws waiting is the review queue's Approve (APPROVE_WAITS, from 0.13.0)",
    "A token's expiry stays readable.": "not drawn: no scene draws a token, its expiry or text the redactor hides",
    "One crafted text no longer slows every key check after it.": "not drawn: a key check's speed isn't something a scene shows",
  },
};
