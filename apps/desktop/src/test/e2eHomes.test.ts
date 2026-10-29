import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  rmSync,
  utimesSync,
  writeFileSync,
  type RmOptions,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FAILED_MARK, REMOVE_HOME, REMOVE_TRIES, cleanHomes, endRun } from "../../e2e/support/homes";

let root = "";
afterEach(() => {
  if (root) rmSync(root, { recursive: true, force: true });
});

/** The error rmSync throws when something wrote into a folder it was removing. */
const notEmpty = (path: string) =>
  Object.assign(new Error(`ENOTEMPTY: directory not empty, rmdir '${path}'`), { code: "ENOTEMPTY" });

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

  it("every removal asks Node to try its rmdir again on ENOTEMPTY, EBUSY and EPERM: 5 times, 200 ms more apart each", () => {
    root = mkdtempSync(join(tmpdir(), "e2e-homes-"));
    home("old1", 400);
    home("old2", 300);
    home("old3", 200);
    home("old4", 100);
    const removed: Array<[string, RmOptions]> = [];
    const remove = (path: string, options: RmOptions) => {
      removed.push([path, options]);
      rmSync(path, options);
    };
    cleanHomes(home("pass"), false, false, 3, remove);
    cleanHomes(home("now"), true, false, 3, remove);
    // A passing run's home, then the failed ones past the newest three kept.
    expect(removed.map(([path]) => path)).toEqual([
      join(root, "tandem-e2e-pass"),
      join(root, "tandem-e2e-old2"),
      join(root, "tandem-e2e-old1"),
    ]);
    // A handle that closes late fails the rmdir, which rmSync retries with these options.
    for (const [, options] of removed) expect(options).toBe(REMOVE_HOME);
    expect(REMOVE_HOME).toEqual({ recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
    expect(existsSync(join(root, "tandem-e2e-pass"))).toBe(false);
  });

  it("D-2625 a removal that throws ENOTEMPTY runs again and walks the home anew, so what's written after the walk goes too", () => {
    root = mkdtempSync(join(tmpdir(), "e2e-homes-"));
    const passed = home("pass");
    const tries: string[] = [];
    // The web app's dev server writes Next.js's config into $HOME as it
    // stops, after rmSync's walk: every retry of its rmdir fails.
    const late = (path: string, options: RmOptions) => {
      tries.push(path);
      if (tries.length === 1) {
        rmSync(join(path, "app.log"));
        mkdirSync(join(path, "home/.config/nextjs-nodejs"), { recursive: true });
        writeFileSync(join(path, "home/.config/nextjs-nodejs/config.json"), "{}");
        throw notEmpty(path);
      }
      rmSync(path, options);
    };
    const warn = vi.fn();
    expect(endRun(passed, 0, false, false, warn, late)).toBe(0);
    expect(tries).toEqual([passed, passed]);
    expect(existsSync(passed)).toBe(false);
    expect(warn).not.toHaveBeenCalled();
    // An error that trying again can't fix isn't retried.
    const denied = vi.fn((path: string) => {
      throw Object.assign(new Error(`EACCES: permission denied, rmdir '${path}'`), { code: "EACCES" });
    });
    expect(endRun(home("denied"), 0, false, false, warn, denied)).toBe(0);
    expect(denied).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalledTimes(1);
  });

  it("D-2625 a removal that fails is a warning naming its own folder, and the homes after it still go", () => {
    root = mkdtempSync(join(tmpdir(), "e2e-homes-"));
    home("old1", 400);
    home("old2", 300);
    home("old3", 200);
    home("old4", 100);
    const now = home("now");
    // Newest first, the two past the three kept are old2, then old1.
    const old2 = join(root, "tandem-e2e-old2");
    const remove = (path: string, options: RmOptions) => {
      if (path === old2) throw notEmpty(path);
      rmSync(path, options);
    };
    const warn = vi.fn();
    expect(endRun(now, 2, true, false, warn, remove)).toBe(1);
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]?.[0])).toContain(old2);
    expect(String(warn.mock.calls[0]?.[0])).not.toContain(now);
    expect(existsSync(join(root, "tandem-e2e-old1"))).toBe(false);
    expect(existsSync(join(now, FAILED_MARK))).toBe(true);
  });

  it("a cleanup that still fails is a warning naming the folder, and the run keeps its exit code", () => {
    root = mkdtempSync(join(tmpdir(), "e2e-homes-"));
    const passed = home("pass");
    // Not empty however often it runs.
    const stuck = vi.fn((path: string) => {
      throw notEmpty(path);
    });
    const warn = vi.fn();
    // Every spec passed and the checks found nothing: the run passes.
    expect(endRun(passed, 0, false, false, warn, stuck)).toBe(0);
    expect(stuck).toHaveBeenCalledTimes(REMOVE_TRIES);
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]?.[0])).toContain(passed);
    expect(String(warn.mock.calls[0]?.[0])).toContain("ENOTEMPTY");
    // A failing run stays failing, whatever its cleanup does.
    expect(endRun(home("failed"), 0, true, false, warn, stuck)).toBe(1);
    expect(endRun(home("specs-failed"), 3, false, false, warn, stuck)).toBe(3);
    // A cleanup that works says nothing.
    warn.mockClear();
    expect(endRun(home("clean"), 0, false, false, warn)).toBe(0);
    expect(warn).not.toHaveBeenCalled();
    expect(existsSync(join(root, "tandem-e2e-clean"))).toBe(false);
  });
});
