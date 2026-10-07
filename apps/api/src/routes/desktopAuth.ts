// Desktop sign-in, server side (F-BE-02, ARCHITECTURE §14.3).
import { and, eq, isNull } from "drizzle-orm";
import { Hono } from "hono";
import { z } from "zod";
import { ACCESS_TTL_SECONDS, signAccessToken } from "../auth/jwt";
import type { Deps } from "../deps";
import { desktopLoginCodes, refreshTokens } from "../db/schema";
import { newId, randomToken, sha256B64url, sha256Hex } from "../lib/ids";
import { ApiError } from "../lib/problem";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { parseJson } from "../lib/validate";

export const CODE_TTL_MS = 5 * 60_000;
export const REFRESH_TTL_MS = 30 * 86_400_000;
export const REDIRECT_URI = /^(http:\/\/127\.0\.0\.1:\d{2,5}\/callback|tandem(-dev)?:\/\/auth\/callback)$/;
const PLATFORMS = ["macos", "windows", "linux"] as const;

export const authorizeBody = z.object({
  challenge: z.string().min(43).max(128),
  state: z.string().min(16).max(128),
  redirect_uri: z
    .string()
    .regex(REDIRECT_URI, "redirect_uri must be the app's loopback or tandem:// callback"),
  device_fingerprint: z.string().min(32).max(256),
  device_name: z.string().max(100).optional(),
  platform: z.enum(PLATFORMS),
});
export const exchangeBody = z.object({
  code: z.string().min(16).max(128),
  verifier: z.string().min(43).max(128),
  device_fingerprint: z.string().min(32).max(256),
});
export const refreshBody = z.object({ refresh_token: z.string().min(16).max(256) });
export const tokenResponse = z.object({
  access_token: z.string(),
  refresh_token: z.string(),
  token_type: z.literal("Bearer"),
  expires_in: z.number(),
});

function invalidGrant(detail: string): ApiError {
  return new ApiError(400, "invalid_grant", "Bad Request", detail, { error: "invalid_grant" });
}

async function issueTokens(
  deps: Deps,
  userId: string,
  familyId: string,
  device: { fingerprint: string; name: string | null; platform: string | null; appVersion: string | null },
) {
  const refresh = randomToken(32);
  const now = deps.clock.now();
  await deps.db.insert(refreshTokens).values({
    id: newId("rt"),
    familyId,
    userId,
    tokenHash: sha256Hex(refresh),
    deviceFingerprint: device.fingerprint,
    deviceName: device.name,
    platform: device.platform,
    appVersion: device.appVersion,
    createdAt: now,
    lastUsedAt: now,
    expiresAt: new Date(now.getTime() + REFRESH_TTL_MS),
  });
  const access = await signAccessToken(deps.keys, { sub: userId, sid: familyId }, deps.clock.nowSec());
  return {
    access_token: access,
    refresh_token: refresh,
    token_type: "Bearer" as const,
    expires_in: ACCESS_TTL_SECONDS,
  };
}

export async function revokeFamily(deps: Deps, familyId: string): Promise<void> {
  await deps.db
    .update(refreshTokens)
    .set({ revokedAt: deps.clock.now() })
    .where(and(eq(refreshTokens.familyId, familyId), isNull(refreshTokens.revokedAt)));
}

export function desktopAuthRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();

  app.post("/authorize", requireUser("cookie"), async (c) => {
    const body = await parseJson(c, authorizeBody);
    const user = c.get("user");
    if (!user) throw new ApiError(401, "UNAUTHENTICATED", "Unauthorized");
    const code = randomToken(32);
    await deps.db.insert(desktopLoginCodes).values({
      codeHash: sha256Hex(code),
      userId: user.id,
      challenge: body.challenge,
      redirectUri: body.redirect_uri,
      deviceFingerprint: body.device_fingerprint,
      deviceName: body.device_name ?? null,
      platform: body.platform,
      expiresAt: new Date(deps.clock.nowMs() + CODE_TTL_MS),
    });
    return c.json({ code });
  });

  app.post("/exchange", async (c) => {
    const body = await parseJson(c, exchangeBody);
    const [row] = await deps.db
      .select()
      .from(desktopLoginCodes)
      .where(eq(desktopLoginCodes.codeHash, sha256Hex(body.code)))
      .limit(1);
    if (!row || row.usedAt || row.expiresAt.getTime() < deps.clock.nowMs())
      throw invalidGrant("The sign-in code expired. Sign in again.");
    if (sha256B64url(body.verifier) !== row.challenge)
      throw invalidGrant("The sign-in code doesn't match this app.");
    if (body.device_fingerprint !== row.deviceFingerprint)
      throw invalidGrant("The sign-in code was issued to another device.");
    const used = await deps.db
      .update(desktopLoginCodes)
      .set({ usedAt: deps.clock.now() })
      .where(and(eq(desktopLoginCodes.codeHash, row.codeHash), isNull(desktopLoginCodes.usedAt)))
      .returning({ codeHash: desktopLoginCodes.codeHash });
    if (used.length === 0) throw invalidGrant("The sign-in code was already used.");
    const tokens = await issueTokens(deps, row.userId, newId("fam"), {
      fingerprint: row.deviceFingerprint,
      name: row.deviceName,
      platform: row.platform,
      appVersion: c.get("client").version ?? null,
    });
    return c.json(tokens);
  });

  app.post("/refresh", async (c) => {
    const body = await parseJson(c, refreshBody);
    const [row] = await deps.db
      .select()
      .from(refreshTokens)
      .where(eq(refreshTokens.tokenHash, sha256Hex(body.refresh_token)))
      .limit(1);
    const unauthenticated = new ApiError(
      401,
      "UNAUTHENTICATED",
      "Unauthorized",
      "Your session ended. Sign in again.",
    );
    if (!row || row.revokedAt || row.expiresAt.getTime() < deps.clock.nowMs()) throw unauthenticated;
    if (row.rotatedAt) {
      // A rotated token came back: someone else may hold it. End the whole family (AC3).
      await revokeFamily(deps, row.familyId);
      throw unauthenticated;
    }
    const rotated = await deps.db
      .update(refreshTokens)
      .set({ rotatedAt: deps.clock.now(), lastUsedAt: deps.clock.now() })
      .where(and(eq(refreshTokens.id, row.id), isNull(refreshTokens.rotatedAt)))
      .returning({ id: refreshTokens.id });
    if (rotated.length === 0) {
      await revokeFamily(deps, row.familyId);
      throw unauthenticated;
    }
    const tokens = await issueTokens(deps, row.userId, row.familyId, {
      fingerprint: row.deviceFingerprint,
      name: row.deviceName,
      platform: row.platform,
      appVersion: c.get("client").version ?? row.appVersion,
    });
    return c.json(tokens);
  });

  app.post("/logout", requireUser("bearer"), async (c) => {
    const body = await parseJson(c, refreshBody);
    const [row] = await deps.db
      .select({ familyId: refreshTokens.familyId, userId: refreshTokens.userId })
      .from(refreshTokens)
      .where(eq(refreshTokens.tokenHash, sha256Hex(body.refresh_token)))
      .limit(1);
    if (row && row.userId === c.get("user")?.id) await revokeFamily(deps, row.familyId);
    return c.body(null, 204);
  });

  return app;
}
