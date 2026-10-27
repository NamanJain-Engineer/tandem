// What the demo's still shows (D-2606, D-2609): its alt texts are built from
// the sample world the demo opens on (story.json), so a retake that renames
// a worktree or an engine can't leave them describing another frame.
import story from "../../desktop/src/demo/story.json";

type Worktree = (typeof story.worktrees)[number];
const ENGINES: Record<string, { label: string } | undefined> = story.engines;

function inState(state: string): Worktree {
  const w = story.worktrees.find((x) => x.state === state);
  if (!w) throw new Error(`story.json has no ${state} worktree`);
  return w;
}

const engine = (w: Worktree) => ENGINES[w.engine]?.label ?? w.engine;
const working = inState("working");
const done = inState("done");
const waiting = inState("waiting");
const frame = `the ${story.repo} repository's ${working.slug} worktree with ${engine(working)} at work beside a shell; in the rail, ${engine(done)} is done in ${done.slug} and ${engine(waiting)} is waiting on you in ${waiting.slug}`;

/** The home page's still (DemoStage), until the live demo loads over it. */
export const STILL_ALT = `Vibezeo's Code mode in the demo: ${frame}, with a tip pointing to it.`;

/** /demo/'s still, a link that launches the demo. */
export const LAUNCH_ALT = `Launch the demo: Vibezeo's Code mode on ${frame}.`;
