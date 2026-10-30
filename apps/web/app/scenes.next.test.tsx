import { describe, expect, it, vi } from "vitest";
import type * as Release from "../lib/release";

// The product scenes at the release their latest slice draws (F-WB-01 AC9,
// D-128): every scene test again, with the site showing SCENES_SLICE. The
// site ships the release before it, so this run is what holds the next
// release's scenes until release.ts names it (OPERATING §6, the
// coordinator's S1 on 23cf1fa0).

vi.mock("../lib/release", async (importOriginal) => {
  const real = await importOriginal<typeof Release>();
  return { ...real, RELEASE: { ...real.RELEASE, version: real.SCENES_SLICE } };
});

const real = await vi.importActual<typeof Release>("../lib/release");

if (real.RELEASE.version === real.SCENES_SLICE) {
  describe("F-WB-01 AC9 the scenes at their latest slice's release", () => {
    it("F-WB-01 AC9 is scenes.test.tsx's own run, since the site shows that release", () => {
      expect(real.RELEASE.version).toBe(real.SCENES_SLICE);
    });
  });
} else {
  const { RELEASE } = await import("../lib/release");
  expect(RELEASE.version).toBe(real.SCENES_SLICE);
  await import("./scenes.test");
}
