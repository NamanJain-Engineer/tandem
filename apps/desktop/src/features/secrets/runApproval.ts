/** What a `vibezeo run` approval's detail holds (F-TL-12): the exact line,
 * where it runs and every variable it gets from `.env`. */
export type RunApproval = {
  command: string;
  cwd: string;
  vars: Array<{ name: string; secret: string | null }>;
  /** A rule covers the command, but `.env` changed since it was allowed. */
  envChanged: boolean;
};

export function parseRunApproval(detail: string): RunApproval | null {
  try {
    const run = (JSON.parse(detail) as { vibezeoRun?: RunApproval }).vibezeoRun;
    return run && typeof run.command === "string" ? run : null;
  } catch {
    return null;
  }
}
