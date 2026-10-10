// Refunds reservations nobody settled within 15 minutes (F-BE-05 AC2): a
// relay that crashed mid-call never keeps a user's credits.
import type { Deps } from "../deps";
import { logger } from "../lib/logger";
import { notifyCredits } from "../services/credits";
import { expiredReservations, refund } from "../services/ledger";
import { withLock } from "./lock";

export const SWEEP_INTERVAL_MS = 60_000;
const LOCK = "job:reservation-sweeper";

/** One pass; the number of reservations refunded, or null when another replica holds the lock. */
export async function sweepReservations(deps: Deps): Promise<number | null> {
  return withLock(deps.redis, LOCK, SWEEP_INTERVAL_MS, async () => {
    const ids = await expiredReservations(deps.db, deps.clock.now());
    let swept = 0;
    for (const id of ids) {
      const done = await refund(deps.db, id, "expire_sweep");
      if (!done) continue;
      swept++;
      await notifyCredits(deps, done.owner);
    }
    if (swept > 0) logger.info({ swept }, "expired reservations refunded");
    return swept;
  });
}

/** Starts the sweeper; the returned function stops it. */
export function startReservationSweeper(deps: Deps): () => void {
  const timer = setInterval(() => {
    sweepReservations(deps).catch((err: unknown) =>
      logger.error({ err: String(err) }, "reservation sweep failed"),
    );
  }, SWEEP_INTERVAL_MS);
  timer.unref();
  return () => clearInterval(timer);
}
