import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { FAILED_MARK, cleanHomes } from "../../e2e/support/homes";

let root = "";
afterEach(() => {
  if (root) rmSync(root, { recursive: true, force: true });
});

/** A run's home in `root`, with a log in it, marked failed `ago` seconds ago if given. */
function home(name: string, failedAgo?: number): string {
  const h = join(root, `tandem-e2e-${name}`);
  mkdirSync(h);
  writeFileSync(join(h, "app.log"), "log");
  if (failedAgo !== undefined) {
    writeFileSync(join(h, FAILED_MARK), "");
    const at = Date.now() / 1000 - failedAgo;
    utimesSync(join(h, FAILED_MARK), at, at);
  }
  return h;
}

describe("the E2E runner's temp homes (OPERATING, E2E lessons)", () => {
  it("a passing run's home is deleted, unless it's asked to keep it", () => {
    root = mkdtempSync(join(tmpdir(), "e2e-homes-"));
    const passed = home("pass");
    cleanHomes(passed, false);
    expect(existsSync(passed)).toBe(false);
    const kept = home("keep");
    cleanHomes(kept, false, true);
    expect(existsSync(join(kept, "app.log"))).toBe(true);
  });

  it("a failing run's home stays, marked, and only the newest three failed homes are kept", () => {
    root = mkdtempSync(join(tmpdir(), "e2e-homes-"));
    home("old1", 400);
    home("old2", 300);
    home("old3", 200);
    // Neither a run in progress nor a home from before the rule is marked: both stay.
    const running = home("running");
    const unmarked = join(root, "tandem-e2e-m7-9-1700000000000");
    mkdirSync(unmarked);
    const failed = home("now");
    cleanHomes(failed, true);
    expect(existsSync(join(failed, FAILED_MARK))).toBe(true);
    expect(readdirSync(root).sort()).toEqual(
      ["tandem-e2e-m7-9-1700000000000", "tandem-e2e-now", "tandem-e2e-old2", "tandem-e2e-old3", "tandem-e2e-running"].sort(),
    );
    expect(existsSync(running)).toBe(true);
  });
});
