// Settles Conductor calls nobody settled (F-BE-08): an app that crashed or
// lost its network never leaves a call open. Each pass takes calls past their
// 30-minute cap and a 2-minute grace, at the provider's reported seconds, or
// at the cap within the hold.
import type { Deps } from "../deps";
import { logger } from "../lib/logger";
import { overdueSessions, sweepSession } from "../services/voice";
import { withLock } from "./lock";

export const VOICE_SWEEP_INTERVAL_MS = 60_000;
const LOCK = "job:voice-sweeper";

/** One pass; the number of calls it settled, or null when another replica holds the lock. */
export async function sweepVoiceSessions(deps: Deps): Promise<number | null> {
  return withLock(deps.redis, LOCK, VOICE_SWEEP_INTERVAL_MS, async () => {
    let swept = 0;
    let failed = 0;
    for (const row of await overdueSessions(deps)) {
      try {
        if (await sweepSession(deps, row)) swept++;
      } catch {
        // Retried on the next pass; the others still go now.
        failed++;
      }
    }
    if (swept + failed > 0) logger.info({ swept, failed }, "unsettled voice calls settled");
    return swept;
  });
}

/** Starts the sweeper; the returned function stops it. */
export function startVoiceSweeper(deps: Deps): () => void {
  const timer = setInterval(() => {
    sweepVoiceSessions(deps).catch((err: unknown) =>
      logger.error({ err: err instanceof Error ? err.name : "unknown" }, "voice sweep failed"),
    );
  }, VOICE_SWEEP_INTERVAL_MS);
  timer.unref();
  return () => clearInterval(timer);
}
