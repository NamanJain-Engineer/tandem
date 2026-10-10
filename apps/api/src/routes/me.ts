import { and, desc, eq, isNull } from "drizzle-orm";
import { Hono } from "hono";
import { streamSSE } from "hono/streaming";
import { z } from "zod";
import type { Deps } from "../deps";
import { cancelOwnedTeams, ownsTeamWithMembers } from "../jobs/deletionJob";
import { refreshTokens, session, user } from "../db/schema";
import { ApiError } from "../lib/problem";
import { parseJson } from "../lib/validate";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import { buildMe, latestSubscription } from "../services/entitlements";
import { revokeFamily } from "./desktopAuth";

export const HEARTBEAT_MS = 25_000;

export function meRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();

  app.get("/", requireUser(), async (c) => {
    const me = await buildMe(deps, c.get("user")?.id ?? "");
    if (!me) throw new ApiError(401, "UNAUTHENTICATED", "Unauthorized", "Sign in to continue.");
    return c.json(me);
  });

  // Plan and credit invalidations for the desktop (F-BE-04).
  app.get("/events", requireUser(), (c) => {
    const userId = c.get("user")?.id ?? "";
    const res = streamSSE(c, async (stream) => {
      const stop = await deps.events.listen(userId, (kind) => {
        void stream.writeSSE({ event: kind, data: JSON.stringify({ kind }) });
      });
      stream.onAbort(stop);
      await stream.writeSSE({ event: "ready", data: "{}" });
      while (!stream.aborted) {
        await stream.sleep(HEARTBEAT_MS);
        if (!stream.aborted) await stream.writeSSE({ event: "heartbeat", data: "{}" });
      }
      stop();
    });
    // Proxies (the web app's /v1 rewrite, CDNs) must not buffer or compress the stream.
    res.headers.set("Cache-Control", "no-cache, no-transform");
    res.headers.set("X-Accel-Buffering", "no");
    return res;
  });

  // Desktop sessions (devices) for the portal (F-WB-05).
  app.get("/sessions", requireUser("cookie"), async (c) => {
    const rows = await deps.db
      .select()
      .from(refreshTokens)
      .where(
        and(
          eq(refreshTokens.userId, c.get("user")?.id ?? ""),
          isNull(refreshTokens.revokedAt),
          isNull(refreshTokens.rotatedAt),
        ),
      )
      .orderBy(desc(refreshTokens.lastUsedAt));
    const now = deps.clock.nowMs();
    return c.json(
      rows
        .filter((r) => r.expiresAt.getTime() > now)
        .map((r) => ({
          id: r.familyId,
          deviceName: r.deviceName,
          platform: r.platform,
          appVersion: r.appVersion,
          lastUsedAt: (r.lastUsedAt ?? r.createdAt).toISOString(),
        })),
    );
  });

  app.delete("/sessions/:id", requireUser("cookie"), async (c) => {
    const familyId = c.req.param("id");
    const [owned] = await deps.db
      .select({ id: refreshTokens.id })
      .from(refreshTokens)
      .where(and(eq(refreshTokens.familyId, familyId), eq(refreshTokens.userId, c.get("user")?.id ?? "")))
      .limit(1);
    if (!owned) throw new ApiError(404, "NOT_FOUND", "Not Found", "That device isn't signed in.");
    await revokeFamily(deps, familyId);
    return c.body(null, 204);
  });

  // Account deletion request: completes within 30 days (jobs/deletionJob); the
  // subscription ends and every sign-in is revoked now (F-TL-05).
  app.delete("/", requireUser("cookie"), async (c) => {
    await parseJson(c, z.object({ confirm: z.literal("DELETE") }));
    const userId = c.get("user")?.id ?? "";
    if (await ownsTeamWithMembers(deps.db, userId)) {
      throw new ApiError(
        409,
        "TEAM_HAS_MEMBERS",
        "Conflict",
        "You own a Team with other members. Transfer it or remove them first.",
      );
    }
    // Billing first: if Stripe fails, nothing has changed and the user can try again.
    const sub = await latestSubscription(deps, userId);
    if (sub && sub.status !== "canceled") await deps.billing.cancelNow(sub.stripeSubscriptionId);
    await cancelOwnedTeams(deps, userId);
    await deps.db.update(user).set({ deletionRequestedAt: deps.clock.now() }).where(eq(user.id, userId));
    const families = await deps.db
      .selectDistinct({ familyId: refreshTokens.familyId })
      .from(refreshTokens)
      .where(eq(refreshTokens.userId, userId));
    for (const f of families) await revokeFamily(deps, f.familyId);
    await deps.db.delete(session).where(eq(session.userId, userId));
    return c.json({ deletion_requested_at: deps.clock.now().toISOString(), completes_within_days: 30 }, 202);
  });

  return app;
}
