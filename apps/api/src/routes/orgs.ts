import { Hono } from "hono";
import { z } from "zod";
import type { Deps } from "../deps";
import { ApiError } from "../lib/problem";
import { parseJson } from "../lib/validate";
import { requireUser } from "../middleware/auth";
import type { AppEnv } from "../middleware/context";
import {
  acceptInvite,
  auditLog,
  createOrg,
  invite,
  orgView,
  policySchema,
  removeMember,
  setPolicy,
  setSeats,
  updateMember,
} from "../services/orgs";
import { addDomain, domainBody, removeDomain, removeSso, setSso, ssoBody, ssoView, verifyDomain } from "../services/sso";

export const createOrgBody = z.object({ name: z.string().trim().min(1).max(80) });
export const inviteBody = z.object({
  email: z.string().email().max(254),
  role: z.enum(["admin", "member"]).default("member"),
});
export const acceptBody = z.object({ token: z.string().min(16).max(128) });
export const memberBody = z
  .object({
    role: z.enum(["admin", "member"]).optional(),
    monthly_cap: z.number().int().min(0).max(10_000_000).nullable().optional(),
  })
  .refine((b) => b.role !== undefined || b.monthly_cap !== undefined, "Send a role or a monthly_cap.");
export const seatsBody = z.object({ seats: z.number().int().min(2).max(500) });

/** The Team plan's org management (F-BL-06). Portal cookie; writes need owner or admin. */
export function orgRoutes(deps: Deps) {
  const app = new Hono<AppEnv>();
  const uid = (c: { get: (k: "user") => { id: string } | null }) => c.get("user")?.id ?? "";

  app.post("/", requireUser("cookie"), async (c) => {
    const body = await parseJson(c, createOrgBody);
    const id = await createOrg(deps, uid(c), body.name);
    return c.json(await orgView(deps, id, uid(c)), 201);
  });

  app.post("/invites/accept", requireUser("cookie"), async (c) => {
    const body = await parseJson(c, acceptBody);
    return c.json(await acceptInvite(deps, uid(c), body.token));
  });

  app.get("/:id", requireUser(), async (c) => c.json(await orgView(deps, c.req.param("id"), uid(c))));

  app.post("/:id/invites", requireUser("cookie"), async (c) => {
    const body = await parseJson(c, inviteBody);
    return c.json(await invite(deps, c.req.param("id"), uid(c), body.email, body.role), 201);
  });

  app.patch("/:id/members/:userId", requireUser("cookie"), async (c) => {
    const body = await parseJson(c, memberBody);
    await updateMember(deps, c.req.param("id"), uid(c), c.req.param("userId"), {
      role: body.role,
      monthlyCap: body.monthly_cap,
    });
    return c.body(null, 204);
  });

  app.delete("/:id/members/:userId", requireUser("cookie"), async (c) => {
    await removeMember(deps, c.req.param("id"), uid(c), c.req.param("userId"));
    return c.body(null, 204);
  });

  app.put("/:id/policy", requireUser("cookie"), async (c) => {
    const body = await parseJson(c, policySchema);
    return c.json(await setPolicy(deps, c.req.param("id"), uid(c), body));
  });

  app.put("/:id/seats", requireUser("cookie"), async (c) => {
    const body = await parseJson(c, seatsBody);
    return c.json(await setSeats(deps, c.req.param("id"), uid(c), body.seats));
  });

  // Single sign-on (F-BL-06 P2): owners and admins, on a paid Team.
  app.get("/:id/sso", requireUser("cookie"), async (c) => c.json(await ssoView(deps, c.req.param("id"), uid(c))));

  app.put("/:id/sso", requireUser("cookie"), async (c) => {
    const body = await parseJson(c, ssoBody);
    return c.json(await setSso(deps, c.req.param("id"), uid(c), body));
  });

  app.delete("/:id/sso", requireUser("cookie"), async (c) => {
    await removeSso(deps, c.req.param("id"), uid(c));
    return c.body(null, 204);
  });

  app.post("/:id/sso/domains", requireUser("cookie"), async (c) => {
    const body = await parseJson(c, domainBody);
    return c.json(await addDomain(deps, c.req.param("id"), uid(c), body.domain), 201);
  });

  app.post("/:id/sso/domains/:domain/verify", requireUser("cookie"), async (c) =>
    c.json(await verifyDomain(deps, c.req.param("id"), uid(c), c.req.param("domain").toLowerCase())),
  );

  app.delete("/:id/sso/domains/:domain", requireUser("cookie"), async (c) => {
    await removeDomain(deps, c.req.param("id"), uid(c), c.req.param("domain").toLowerCase());
    return c.body(null, 204);
  });

  app.get("/:id/audit", requireUser("cookie"), async (c) => {
    const cursor = c.req.query("cursor");
    const before = cursor ? new Date(cursor) : undefined;
    if (before && Number.isNaN(before.getTime())) {
      throw new ApiError(422, "VALIDATION", "Unprocessable Entity", "cursor must be a timestamp.");
    }
    return c.json(await auditLog(deps, c.req.param("id"), uid(c), before));
  });

  return app;
}
