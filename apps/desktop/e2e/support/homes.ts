// What happens to an E2E run's temp TANDEM_HOME when the run ends (run.ts).
import { existsSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

/** How many failed runs' homes stay in the temp folder for their logs. */
export const KEEP_FAILED_HOMES = 3;
/** The file that marks a failed run's home, so only those are ever pruned. */
export const FAILED_MARK = "e2e-failed";

/** A passing run's home goes, unless `keep`; a failing run's is marked and
 * kept, and of the marked homes beside it only the newest `keepFailed`
 * stay. Only homes a run marked as failed are pruned, so another run's home
 * in progress, or one left before this rule, is never touched. */
export function cleanHomes(home: string, failed: boolean, keep = false, keepFailed = KEEP_FAILED_HOMES): void {
  if (!failed) {
    if (!keep) rmSync(home, { recursive: true, force: true });
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
  for (const old of marked.slice(keepFailed)) rmSync(old.home, { recursive: true, force: true });
}
