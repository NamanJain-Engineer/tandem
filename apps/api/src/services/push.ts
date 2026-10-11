// Phone push (F-RM-06, D-2020): the desktop sends a payload it sealed for
// the phone, with the phone's push token (which it learned over the
// end-to-end channel). The API keeps neither: it checks the pairing is the
// caller's and live, and forwards the opaque bytes. Logs hold the pairing id,
// platform and outcome only.
import { and, eq, isNull } from "drizzle-orm";
import { z } from "zod";
import type { Deps } from "../deps";
import { pairings } from "../db/schema";
import { logger } from "../lib/logger";
import { ApiError } from "../lib/problem";
import { assertPhoneAllowed } from "./pairings";
import { takeForUser } from "./rateLimit";

/** FCM data messages hold 4 KB; base64url of a sealed alert stays well under. */
export const MAX_PUSH_PAYLOAD = 3000;

export const pushBody = z.object({
  platform: z.enum(["fcm", "apns"]),
  token: z.string().min(8).max(4096),
  payload: z.string().regex(/^[A-Za-z0-9_-]+$/, "payload must be base64url").max(MAX_PUSH_PAYLOAD),
});

export async function forwardPush(
  deps: Deps,
  userId: string,
  pairingId: string,
  body: z.infer<typeof pushBody>,
): Promise<"sent" | "unregistered"> {
  const [row] = await deps.db
    .select({ id: pairings.id })
    .from(pairings)
    .where(and(eq(pairings.id, pairingId), eq(pairings.userId, userId), isNull(pairings.revokedAt)))
    .limit(1);
  if (!row) throw new ApiError(404, "NOT_FOUND", "Not Found", "There's no such pairing.");
  await assertPhoneAllowed(deps, userId);
  const rate = await takeForUser(deps.redis, userId, "push", deps.clock.nowMs());
  if (!rate.allowed) {
    throw new ApiError(429, "RATE_LIMITED", "Too Many Requests", "Too many alerts. Try again shortly.", {
      retry_after_ms: rate.retryAfterMs,
    });
  }
  const result = await deps.push.send(body);
  logger.info({ pairingId, platform: body.platform, result }, "phone push");
  if (result === "unavailable") {
    throw new ApiError(503, "UPSTREAM_UNAVAILABLE", "Service Unavailable", "Push isn't available right now.");
  }
  return result;
}
