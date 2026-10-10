// CLI device-code sign-in (F-BE-03, ARCHITECTURE §14.3), after RFC 8628: a
// device shows a short code, the user approves it on the web while signed
// in, and the device polls for tokens.
import { randomInt } from "node:crypto";
import { and, eq } from "drizzle-orm";
import { Hono } from "hono";
import { z } from "zod";
import type { Deps } from "../deps";
import { deviceCodes } from "../db/schema";
import { newId, randomToken, sha256Hex } from "../lib/ids";
import { ApiError } from "../lib/problem";
import { parseJson } from "../lib/validate";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { clientIp } from "../middleware/rateLimit";
import { take } from "../services/rateLimit";
import { issueTokens } from "./desktopAuth";

/** No vowels (no words) and no look-alikes (0/O, 1/I). */
export const USER_CODE_ALPHABET = "BCDFGHJKLMNPQRSTVWXZ";
export const DEVICE_TTL_MS = 10 * 60_000;
export const POLL_INTERVAL_SEC = 5;
/** `device_start`: 10 an hour per IP (ARCHITECTURE §14.9). */
export const START_LIMIT = { capacity: 10, windowMs: 3_600_000 };

export const startBody = z.object({
  client: z.string().trim().min(1).max(60),
  version: z.string().trim().min(1).max(40),
});
export const approveBody = z.object({ user_code: z.string().min(8).max(20), approve: z.boolean() });
export const tokenBody = z.object({ device_code: z.string().min(16).max(128) });
export const startResponse = z.object({
  device_code: z.string(),
  user_code: z.string(),
  verification_uri: z.string(),
  verification_uri_complete: z.string(),
  interval: z.number(),
  expires_in: z.number(),
});

export function newUserCode(): string {
  const pick = () => USER_CODE_ALPHABET[randomInt(USER_CODE_ALPHABET.length)];
  const chars = Array.from({ length: 8 }, pick).join("");
  return `${chars.slice(0, 4)}-${chars.slice(4)}`;
}

/** "abcd efgh", "ABCD-EFGH" and "abcdefgh" are the same code; anything else is none. */
export function normalizeUserCode(input: string): string | null {
  const letters = input.toUpperCase().replace(/[\s-]/g, "");
  if (letters.length !== 8 || [...letters].some((ch) => !USER_CODE_ALPHABET.includes(ch))) return null;
  return `${letters.slice(0, 4)}-${letters.slice(4)}`;
}

/** The token endpoint's errors: 400 with `error` set, as RFC 8628 has them. */
const oauthError = (error: string, detail: string, extra: Record<string, unknown> = {}) =>
  new ApiError(400, error, "Bad Request", detail, { error, ...extra });

export function deviceAuthRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();

  app.post("/start", async (c) => {
    const body = await parseJson(c, startBody);
    const limit = await take(
      deps.redis,
      `rl:device_start:${clientIp(c)}`,
      START_LIMIT.capacity,
      START_LIMIT.windowMs,
      deps.clock.nowMs(),
    );
    if (!limit.allowed) {
      c.header("Retry-After", String(Math.ceil(limit.retryAfterMs / 1000)));
      throw new ApiError(429, "RATE_LIMITED", "Too Many Requests", "Too many sign-in codes. Try again later.");
    }
    const deviceCode = randomToken(32);
    const expiresAt = new Date(deps.clock.nowMs() + DEVICE_TTL_MS);
    let userCode = "";
    for (let attempt = 0; attempt < 5 && !userCode; attempt++) {
      const candidate = newUserCode();
      const inserted = await deps.db
        .insert(deviceCodes)
        .values({
          deviceCodeHash: sha256Hex(deviceCode),
          userCode: candidate,
          intervalSec: POLL_INTERVAL_SEC,
          expiresAt,
          clientInfo: { client: body.client, version: body.version },
        })
        .onConflictDoNothing()
        .returning({ userCode: deviceCodes.userCode });
      userCode = inserted[0]?.userCode ?? "";
    }
    if (!userCode) throw new ApiError(503, "UNAVAILABLE", "Service Unavailable", "Try again.");
    const uri = `${deps.env.WEB_PUBLIC_URL}/device`;
    return c.json({
      device_code: deviceCode,
      user_code: userCode,
      verification_uri: uri,
      verification_uri_complete: `${uri}?code=${userCode}`,
      interval: POLL_INTERVAL_SEC,
      expires_in: DEVICE_TTL_MS / 1000,
    });
  });

  async function pendingByUserCode(raw: string) {
    const code = normalizeUserCode(raw);
    const notFound = new ApiError(404, "NOT_FOUND", "Not Found", "That code isn't valid or has expired.");
    if (!code) throw notFound;
    const [row] = await deps.db.select().from(deviceCodes).where(eq(deviceCodes.userCode, code)).limit(1);
    if (!row || row.status !== "pending" || row.expiresAt.getTime() <= deps.clock.nowMs()) throw notFound;
    return row;
  }

  // What the approval page shows: which app asks (never anything secret).
  app.get("/info", requireUser("cookie"), async (c) => {
    const row = await pendingByUserCode(c.req.query("user_code") ?? "");
    const info = (row.clientInfo ?? {}) as { client?: string; version?: string };
    return c.json({
      user_code: row.userCode,
      client: info.client ?? "",
      version: info.version ?? "",
      expires_at: row.expiresAt.toISOString(),
    });
  });

  app.post("/approve", requireUser("cookie"), async (c) => {
    const body = await parseJson(c, approveBody);
    const row = await pendingByUserCode(body.user_code);
    const updated = await deps.db
      .update(deviceCodes)
      .set({ status: body.approve ? "approved" : "denied", userId: c.get("user")?.id ?? null })
      .where(and(eq(deviceCodes.deviceCodeHash, row.deviceCodeHash), eq(deviceCodes.status, "pending")))
      .returning({ status: deviceCodes.status });
    if (updated.length === 0) throw new ApiError(404, "NOT_FOUND", "Not Found", "That code was already used.");
    return c.body(null, 204);
  });

  app.post("/token", async (c) => {
    const body = await parseJson(c, tokenBody);
    const hash = sha256Hex(body.device_code);
    const [row] = await deps.db.select().from(deviceCodes).where(eq(deviceCodes.deviceCodeHash, hash)).limit(1);
    if (!row || row.status === "consumed") throw oauthError("invalid_grant", "Unknown or already used code.");
    const now = deps.clock.nowMs();
    if (row.expiresAt.getTime() <= now) throw oauthError("expired_token", "The code expired. Start again.");
    if (row.lastPollAt && now - row.lastPollAt.getTime() < row.intervalSec * 1000) {
      const interval = row.intervalSec + 5;
      await deps.db
        .update(deviceCodes)
        .set({ intervalSec: interval, lastPollAt: new Date(now) })
        .where(eq(deviceCodes.deviceCodeHash, hash));
      throw oauthError("slow_down", `Poll at most every ${interval} seconds.`, { interval });
    }
    await deps.db.update(deviceCodes).set({ lastPollAt: new Date(now) }).where(eq(deviceCodes.deviceCodeHash, hash));
    if (row.status === "pending") throw oauthError("authorization_pending", "Waiting for approval.");
    if (row.status === "denied") throw oauthError("access_denied", "The sign-in was denied.");
    const consumed = await deps.db
      .update(deviceCodes)
      .set({ status: "consumed" })
      .where(and(eq(deviceCodes.deviceCodeHash, hash), eq(deviceCodes.status, "approved")))
      .returning({ userId: deviceCodes.userId });
    const userId = consumed[0]?.userId;
    if (!userId) throw oauthError("invalid_grant", "Unknown or already used code.");
    const info = (row.clientInfo ?? {}) as { client?: string; version?: string };
    return c.json(
      await issueTokens(deps, userId, newId("fam"), {
        fingerprint: `device-code:${hash.slice(0, 32)}`,
        name: info.client ?? "CLI",
        platform: "cli",
        appVersion: info.version ?? null,
      }),
    );
  });

  return app;
}
