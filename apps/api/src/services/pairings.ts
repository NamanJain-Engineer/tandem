// Phone pairings (F-RM-03) and the relay's authentication (F-BE-11,
// ARCHITECTURE §13.3, §14.8). The server stores a hash of each pairing token
// and who owns the pairing; it never sees keys or frame contents.
import { timingSafeEqual } from "node:crypto";
import { and, eq, isNull } from "drizzle-orm";
import { z } from "zod";
import { verifyAccessToken } from "../auth/jwt";
import type { Deps } from "../deps";
import { pairings } from "../db/schema";
import { randomToken, sha256Hex } from "../lib/ids";
import { ApiError } from "../lib/problem";
import type { RelayAuthenticate, RelayPeer } from "../routes/relay";
import { planFor } from "./entitlements";
import { policyOf } from "./orgs";

/** How long the QR code (and so the first handshake) is valid. */
export const PAIRING_QR_TTL_MS = 10 * 60_000;
/** The phone's WebSocket subprotocol carries its pairing token (browsers can't set headers). */
export const PAIR_PROTOCOL_PREFIX = "tandem-pair.";

export const createPairingBody = z.object({
  pairing_id: z.string().regex(/^pr_[A-Za-z0-9_-]{4,64}$/, "pairing_id must look like pr_…"),
  device_label: z.string().trim().min(1).max(60).optional(),
});

/** Phone access needs Pro or Team, and a Team admin can turn it off (F-BL-06). */
export async function assertPhoneAllowed(deps: Deps, userId: string): Promise<void> {
  const { effective, membership } = await planFor(deps, userId);
  if (!effective.limits.phone) {
    throw new ApiError(403, "FORBIDDEN_PLAN", "Forbidden", "This feature is included with Pro.", {
      feature: "phone",
      plan: effective.plan,
    });
  }
  if (membership && effective.plan === "team" && (await policyOf(deps, membership.orgId)).disablePhone) {
    throw new ApiError(403, "FORBIDDEN_POLICY", "Forbidden", "Your team's admin turned the phone companion off.", {
      feature: "phone",
    });
  }
}

export function relayUrlFor(deps: Deps, pairingId: string): string {
  const base = deps.env.API_PUBLIC_URL.replace(/\/+$/, "").replace(/^http/, "ws");
  return `${base}/v1/relay/${pairingId}`;
}

export async function createPairing(
  deps: Deps,
  userId: string,
  body: z.infer<typeof createPairingBody>,
): Promise<{ pairing_token: string; relay_url: string; expires_at: string }> {
  await assertPhoneAllowed(deps, userId);
  const token = randomToken(32);
  const inserted = await deps.db
    .insert(pairings)
    .values({
      id: body.pairing_id,
      userId,
      pairingTokenHash: sha256Hex(token),
      deviceLabel: body.device_label ?? null,
    })
    .onConflictDoNothing()
    .returning({ id: pairings.id });
  if (inserted.length === 0) {
    throw new ApiError(409, "PAIRING_EXISTS", "Conflict", "That pairing id is already taken.");
  }
  return {
    pairing_token: token,
    relay_url: relayUrlFor(deps, body.pairing_id),
    expires_at: new Date(deps.clock.nowMs() + PAIRING_QR_TTL_MS).toISOString(),
  };
}

/** Marks the pairing revoked and drops its relay sockets. Idempotent for the owner. */
export async function revokePairing(deps: Deps, userId: string, pairingId: string): Promise<void> {
  const [row] = await deps.db
    .select({ revokedAt: pairings.revokedAt })
    .from(pairings)
    .where(and(eq(pairings.id, pairingId), eq(pairings.userId, userId)))
    .limit(1);
  if (!row) throw new ApiError(404, "NOT_FOUND", "Not Found", "There's no such pairing.");
  if (!row.revokedAt) {
    await deps.db.update(pairings).set({ revokedAt: deps.clock.now() }).where(eq(pairings.id, pairingId));
  }
  deps.relay.drop(pairingId);
}

/**
 * Who may open `/v1/relay/:pairing_id`: the owner's desktop (bearer) or the
 * phone holding the pairing token, while the pairing is live and the plan
 * still includes phone access.
 */
export function relayAuthenticator(deps: Deps): RelayAuthenticate {
  return async (pairingId, req): Promise<RelayPeer | null> => {
    const [row] = await deps.db
      .select()
      .from(pairings)
      .where(and(eq(pairings.id, pairingId), isNull(pairings.revokedAt)))
      .limit(1);
    if (!row) return null;
    let peer: RelayPeer | null = null;
    if (req.bearer) {
      const claims = await verifyAccessToken(deps.keys, req.bearer, deps.clock.nowSec());
      if (claims?.sub === row.userId) peer = { role: "desktop" };
    } else {
      const protocol = req.protocols.find((p) => p.startsWith(PAIR_PROTOCOL_PREFIX));
      const token = protocol?.slice(PAIR_PROTOCOL_PREFIX.length);
      if (protocol && token && sameHex(sha256Hex(token), row.pairingTokenHash)) peer = { role: "phone", protocol };
    }
    if (!peer) return null;
    try {
      await assertPhoneAllowed(deps, row.userId);
    } catch {
      return null;
    }
    return peer;
  };
}

function sameHex(a: string, b: string): boolean {
  const x = Buffer.from(a, "hex");
  const y = Buffer.from(b, "hex");
  return x.length === y.length && timingSafeEqual(x, y);
}
