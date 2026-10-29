// What happens to an E2E run's temp TANDEM_HOME when the run ends (run.ts).
import { existsSync, readdirSync, rmSync, statSync, writeFileSync, type RmOptions } from "node:fs";
import { dirname, join } from "node:path";

/** How many failed runs' homes stay in the temp folder for their logs. */
export const KEEP_FAILED_HOMES = 3;
/** The file that marks a failed run's home, so only those are ever pruned. */
export const FAILED_MARK = "e2e-failed";

/** How a home is removed. On ENOTEMPTY, EBUSY or EPERM, rmSync tries the
 * folder's own rmdir again, up to 5 times, 200 ms more apart each. */
export const REMOVE_HOME: RmOptions = { recursive: true, force: true, maxRetries: 5, retryDelay: 200 };

/** How many times a home's whole removal runs. Node 22's rmSync retries only
 * the rmdir, never the walk, so whatever is written into the home after the
 * walk fails every retry (D-2625). Each try here walks the home again. */
export const REMOVE_TRIES = 3;

const RETRIED = new Set(["ENOTEMPTY", "EBUSY", "EPERM"]);

/** Removes a folder; rmSync, unless a test passes its own. */
export type Remove = (path: string, options: RmOptions) => void;

/** Removes a home, running the whole removal again while it fails with
 * ENOTEMPTY, EBUSY or EPERM, 200 ms more apart each time. */
function removeHome(path: string, remove: Remove): void {
  for (let attempt = 1; ; attempt++) {
    try {
      remove(path, REMOVE_HOME);
      return;
    } catch (e) {
      const code = (e as NodeJS.ErrnoException).code ?? "";
      if (attempt >= REMOVE_TRIES || !RETRIED.has(code)) throw e;
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, attempt * 200);
    }
  }
}

/** A passing run's home goes, unless `keep`; a failing run's is marked and
 * kept, and of the marked homes beside it only the newest `keepFailed`
 * stay. Only homes a run marked as failed are pruned, so another run's home
 * in progress, or one left before this rule, is never touched. */
export function cleanHomes(
  home: string,
  failed: boolean,
  keep = false,
  keepFailed = KEEP_FAILED_HOMES,
  remove: Remove = rmSync,
): void {
  if (!failed) {
    if (!keep) removeHome(home, remove);
    return;
  }
  writeFileSync(join(home, FAILED_MARK), new Date().toISOString());
  const dir = dirname(home);
  const marked = readdirSync(dir)
    .filter((n) => n.startsWith("tandem-e2e-"))
    .map((n) => join(dir, n, FAILED_MARK))
    .filter((mark) => existsSync(mark))
    .map((mark) => ({ home: dirname(mark), at: statSync(mark).mtimeMs }))
    .sort((a, b) => b.at - a.at);
  for (const old of marked.slice(keepFailed)) removeHome(old.home, remove);
}

/** run.ts's last step: the home is cleaned up as `cleanHomes` says, and the
 * run's exit code returned. A cleanup that still fails is a warning naming
 * the folder, never the run's result: a run whose specs all passed keeps
 * its passing code. */
export function endRun(
  home: string,
  code: number,
  failed: boolean,
  keep: boolean,
  warn: (message: string) => void = console.warn,
  remove: Remove = rmSync,
): number {
  try {
    cleanHomes(home, failed, keep, KEEP_FAILED_HOMES, remove);
  } catch (e) {
    warn(`E2E: couldn't remove ${home} (${e instanceof Error ? e.message : String(e)}); delete it by hand.`);
  }
  return failed ? 1 : code;
}
