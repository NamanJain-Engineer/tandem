import { describe, expect, it } from "vitest";
import story from "../../desktop/src/demo/story.json";
import { LAUNCH_ALT, STILL_ALT } from "./demoStill";

const ENGINES: Record<string, { label: string } | undefined> = story.engines;

describe("D-2609 the demo still's alts say what its first frame shows", () => {
  it("D-2609 they name the repository, each worktree and the engine in it, from the demo's sample world", () => {
    for (const alt of [STILL_ALT, LAUNCH_ALT]) {
      expect(alt).toContain(`the ${story.repo} repository`);
      for (const w of story.worktrees) {
        expect(alt, w.slug).toContain(w.slug);
        expect(alt, w.slug).toContain(ENGINES[w.engine]?.label ?? "?");
      }
    }
    expect(LAUNCH_ALT).toMatch(/^Launch the demo: /);
  });
});
