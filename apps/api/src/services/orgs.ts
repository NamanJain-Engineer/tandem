// The Team plan (F-BL-06, ARCHITECTURE §14.7): orgs, invites, roles, seats,
// per-member caps, admin policy and the org audit log (F-TL-10).
import { and, desc, eq, gt, isNull, lt, sql } from "drizzle-orm";
import { PRICES } from "@tandem/protocol/plans";
import {
  CLIENT_AUDIT_EVENTS,
  DEFAULT_POLICY,
  type ClientAuditEvent,
  type OrgPolicy,
  type OrgRole,
} from "@tandem/protocol/team";
import { z } from "zod";
import type { Deps } from "../deps";
import { auditEvents, orgInvites, orgMembers, orgPolicies, orgs, subscriptions, user } from "../db/schema";
import { newId, randomToken, sha256Hex } from "../lib/ids";
import { logger } from "../lib/logger";
import { ApiError } from "../lib/problem";
import { balance, memberSpend } from "./ledger";
import { membershipOf } from "./membership";
import { effectivePlan, type SubscriptionRow } from "./plan";

export const INVITE_TTL_MS = 7 * 86_400_000;

export const policySchema = z.object({
  allowedEngines: z.array(z.string().min(1).max(40)).max(50).nullable(),
  allowedPlugins: z.array(z.string().min(1).max(60)).max(200).nullable(),
  maxAccess: z.enum(["read", "edits", "full"]),
  forceTelemetryOff: z.boolean(),
  disableAuto: z.boolean(),
  disableConductor: z.boolean(),
  disablePhone: z.boolean(),
  auditEnabled: z.boolean(),
});

const forbidden = (detail = "Only the organization's owner or admins can do that.") =>
  new ApiError(403, "FORBIDDEN", "Forbidden", detail);
const notFound = () => new ApiError(404, "NOT_FOUND", "Not Found", "There's no such organization.");

/** Writes one audit row. Metadata only: ids, names of settings, counts, never content. */
export async function audit(
  deps: Deps,
  orgId: string,
  actorUserId: string | null,
  event: string,
  meta: Record<string, string | number | boolean | null> = {},
): Promise<void> {
  await deps.db.insert(auditEvents).values({ id: newId("aud"), orgId, actorUserId, event, meta });
}

export async function policyOf(deps: Deps, orgId: string): Promise<OrgPolicy> {
  const [row] = await deps.db.select().from(orgPolicies).where(eq(orgPolicies.orgId, orgId)).limit(1);
  const parsed = policySchema.safeParse({
    ...DEFAULT_POLICY,
    ...((row?.policy as object | undefined) ?? {}),
  });
  return parsed.success ? parsed.data : DEFAULT_POLICY;
}

export async function orgSubscription(deps: Deps, orgId: string) {
  const [sub] = await deps.db
    .select()
    .from(subscriptions)
    .where(and(eq(subscriptions.ownerKind, "org"), eq(subscriptions.ownerId, orgId)))
    .orderBy(desc(subscriptions.updatedAt))
    .limit(1);
  return sub ?? null;
}

/** The org's Team plan, recomputed from the database like any plan. */
export async function orgPlan(deps: Deps, orgId: string) {
  const sub = await orgSubscription(deps, orgId);
  return { sub, effective: effectivePlan(sub as SubscriptionRow | null, deps.clock.now()) };
}

async function memberIds(deps: Deps, orgId: string): Promise<string[]> {
  const rows = await deps.db
    .select({ id: orgMembers.userId })
    .from(orgMembers)
    .where(eq(orgMembers.orgId, orgId));
  return rows.map((r) => r.id);
}

/** Tells every member's desktop to refetch `/me` (plan, policy). */
export async function notifyOrg(deps: Deps, orgId: string, kind: "entitlement" | "credits" = "entitlement") {
  for (const id of await memberIds(deps, orgId)) await deps.events.publish(id, kind);
}

/** The caller's role in `orgId`, or 403/404. */
export async function roleIn(deps: Deps, orgId: string, userId: string, need: "admin" | "member"): Promise<OrgRole> {
  const [org] = await deps.db.select({ id: orgs.id }).from(orgs).where(eq(orgs.id, orgId)).limit(1);
  if (!org) throw notFound();
  const m = await membershipOf(deps.db, userId);
  if (!m || m.orgId !== orgId) throw notFound();
  if (need === "admin" && m.role === "member") throw forbidden();
  return m.role;
}

/** Creates an org with the caller as its owner. One org per user (D-606). */
export async function createOrg(deps: Deps, userId: string, name: string) {
  if (await membershipOf(deps.db, userId)) {
    throw new ApiError(409, "CONFLICT", "Conflict", "You already belong to an organization.");
  }
  const id = newId("org");
  await deps.db.transaction(async (tx) => {
    await tx.insert(orgs).values({ id, name, ownerId: userId });
    await tx.insert(orgMembers).values({ orgId: id, userId, role: "owner" });
    await tx.insert(orgPolicies).values({ orgId: id, policy: DEFAULT_POLICY, updatedAt: deps.clock.now() });
  });
  await audit(deps, id, userId, "org_created");
  return id;
}

/** The org the caller owns or administers, created when they have none (Team checkout). */
export async function orgForCheckout(deps: Deps, userId: string): Promise<string> {
  const m = await membershipOf(deps.db, userId);
  if (m) {
    if (m.role === "member") throw forbidden("Only the organization's owner or admins can change its plan.");
    return m.orgId;
  }
  const [u] = await deps.db
    .select({ name: user.name, email: user.email })
    .from(user)
    .where(eq(user.id, userId))
    .limit(1);
  const base = (u?.name?.trim() || u?.email.split("@")[0] || "My").slice(0, 60);
  return createOrg(deps, userId, `${base}'s team`);
}

export type OrgView = Awaited<ReturnType<typeof orgView>>;

export async function orgView(deps: Deps, orgId: string, viewerId: string) {
  const role = await roleIn(deps, orgId, viewerId, "member");
  const [org] = await deps.db.select().from(orgs).where(eq(orgs.id, orgId)).limit(1);
  const { sub, effective } = await orgPlan(deps, orgId);
  const rows = await deps.db
    .select({
      userId: orgMembers.userId,
      role: orgMembers.role,
      monthlyCap: orgMembers.monthlyCap,
      email: user.email,
      name: user.name,
      joinedAt: orgMembers.createdAt,
    })
    .from(orgMembers)
    .innerJoin(user, eq(user.id, orgMembers.userId))
    .where(eq(orgMembers.orgId, orgId))
    .orderBy(orgMembers.createdAt);
  const admin = role !== "member";
  const members = await Promise.all(
    rows
      .filter((r) => admin || r.userId === viewerId)
      .map(async (r) => ({
        userId: r.userId,
        email: r.email,
        name: r.name,
        role: r.role,
        monthlyCap: r.monthlyCap,
        spent: await memberSpend(deps.db, orgId, r.userId),
        joinedAt: r.joinedAt.toISOString(),
      })),
  );
  const invites = admin
    ? await deps.db
        .select({
          id: orgInvites.id,
          email: orgInvites.email,
          role: orgInvites.role,
          expiresAt: orgInvites.expiresAt,
        })
        .from(orgInvites)
        .where(
          and(
            eq(orgInvites.orgId, orgId),
            isNull(orgInvites.acceptedAt),
            gt(orgInvites.expiresAt, deps.clock.now()),
          ),
        )
    : [];
  const b = await balance(deps.db, { kind: "org", id: orgId });
  return {
    id: orgId,
    name: org?.name ?? "",
    role,
    plan: effective.plan,
    status: effective.status,
    seats: sub?.seats ?? null,
    memberCount: rows.length,
    members,
    invites: invites.map((i) => ({ ...i, expiresAt: i.expiresAt.toISOString() })),
    policy: await policyOf(deps, orgId),
    credits: b,
  };
}

export async function seatsInUse(deps: Deps, orgId: string): Promise<number> {
  const [members] = await deps.db
    .select({ n: sql<number>`count(*)::int` })
    .from(orgMembers)
    .where(eq(orgMembers.orgId, orgId));
  const [pending] = await deps.db
    .select({ n: sql<number>`count(*)::int` })
    .from(orgInvites)
    .where(
      and(
        eq(orgInvites.orgId, orgId),
        isNull(orgInvites.acceptedAt),
        gt(orgInvites.expiresAt, deps.clock.now()),
      ),
    );
  return (members?.n ?? 0) + (pending?.n ?? 0);
}

/** Emails an invite link; a seat is held until it's accepted or expires. */
export async function invite(
  deps: Deps,
  orgId: string,
  actorId: string,
  email: string,
  role: "admin" | "member",
) {
  await roleIn(deps, orgId, actorId, "admin");
  const { sub } = await orgPlan(deps, orgId);
  const seats = sub?.seats ?? PRICES.team.minSeats;
  if ((await seatsInUse(deps, orgId)) >= seats) {
    throw new ApiError(409, "SEATS_FULL", "Conflict", `All ${seats} seats are taken. Add seats first.`, {
      seats,
    });
  }
  const normalized = email.trim().toLowerCase();
  const [existing] = await deps.db
    .select({ userId: orgMembers.userId })
    .from(orgMembers)
    .innerJoin(user, eq(user.id, orgMembers.userId))
    .where(and(eq(orgMembers.orgId, orgId), eq(sql`lower(${user.email})`, normalized)))
    .limit(1);
  if (existing) throw new ApiError(409, "CONFLICT", "Conflict", "That person is already a member.");
  const token = randomToken();
  const id = newId("inv");
  await deps.db
    .delete(orgInvites)
    .where(and(eq(orgInvites.orgId, orgId), eq(orgInvites.email, normalized), isNull(orgInvites.acceptedAt)));
  await deps.db.insert(orgInvites).values({
    id,
    orgId,
    email: normalized,
    role,
    tokenHash: sha256Hex(token),
    expiresAt: new Date(deps.clock.nowMs() + INVITE_TTL_MS),
  });
  const [org] = await deps.db.select({ name: orgs.name }).from(orgs).where(eq(orgs.id, orgId)).limit(1);
  await deps.email
    .send({
      to: normalized,
      subject: `You're invited to ${org?.name ?? "a team"} on Vibezeo`,
      text: `You've been invited to join ${org?.name ?? "a team"} on Vibezeo as ${role === "admin" ? "an admin" : "a member"}.\n\nAccept the invite: ${deps.env.WEB_PUBLIC_URL}/team/join?token=${token}\n\nThe link works for 7 days. Sign in or create an account with this email address first.`,
    })
    .catch((err: unknown) => logger.error({ err: String(err) }, "invite email failed"));
  await audit(deps, orgId, actorId, "member_invited", { role });
  return { id };
}

export async function acceptInvite(deps: Deps, userId: string, token: string) {
  const [inv] = await deps.db
    .select()
    .from(orgInvites)
    .where(eq(orgInvites.tokenHash, sha256Hex(token)))
    .limit(1);
  if (!inv || inv.acceptedAt || inv.expiresAt.getTime() <= deps.clock.nowMs()) {
    throw new ApiError(404, "NOT_FOUND", "Not Found", "This invite has expired or was already used.");
  }
  const [u] = await deps.db.select({ email: user.email }).from(user).where(eq(user.id, userId)).limit(1);
  if (!u || u.email.toLowerCase() !== inv.email) {
    throw new ApiError(
      403,
      "FORBIDDEN",
      "Forbidden",
      `This invite is for ${inv.email}. Sign in with that address.`,
    );
  }
  if (await membershipOf(deps.db, userId)) {
    throw new ApiError(409, "CONFLICT", "Conflict", "You already belong to an organization. Leave it first.");
  }
  await deps.db.transaction(async (tx) => {
    await tx
      .insert(orgMembers)
      .values({ orgId: inv.orgId, userId, role: inv.role === "admin" ? "admin" : "member" });
    await tx.update(orgInvites).set({ acceptedAt: deps.clock.now() }).where(eq(orgInvites.id, inv.id));
  });
  await audit(deps, inv.orgId, userId, "member_added", { role: inv.role });
  await deps.events.publish(userId, "entitlement");
  return { orgId: inv.orgId };
}

export async function updateMember(
  deps: Deps,
  orgId: string,
  actorId: string,
  targetId: string,
  change: { role?: "admin" | "member"; monthlyCap?: number | null },
) {
  const actorRole = await roleIn(deps, orgId, actorId, "admin");
  const target = await membershipOf(deps.db, targetId);
  if (!target || target.orgId !== orgId) throw new ApiError(404, "NOT_FOUND", "Not Found", "No such member.");
  if (target.role === "owner" && change.role) throw forbidden("The owner's role can't change.");
  if (change.role === "admin" && actorRole !== "owner") throw forbidden("Only the owner can make admins.");
  const set: Partial<typeof orgMembers.$inferInsert> = {};
  if (change.role) set.role = change.role;
  if (change.monthlyCap !== undefined) set.monthlyCap = change.monthlyCap;
  if (Object.keys(set).length === 0) return;
  await deps.db
    .update(orgMembers)
    .set(set)
    .where(and(eq(orgMembers.orgId, orgId), eq(orgMembers.userId, targetId)));
  await audit(deps, orgId, actorId, "member_changed", {
    member: targetId,
    ...(change.role ? { role: change.role } : {}),
    ...(change.monthlyCap !== undefined ? { monthlyCap: change.monthlyCap } : {}),
  });
  await deps.events.publish(targetId, "credits");
}

/** Removes a member; members may remove themselves (leave), the owner can't be removed. */
export async function removeMember(deps: Deps, orgId: string, actorId: string, targetId: string) {
  if (actorId !== targetId) await roleIn(deps, orgId, actorId, "admin");
  const target = await membershipOf(deps.db, targetId);
  if (!target || target.orgId !== orgId) throw new ApiError(404, "NOT_FOUND", "Not Found", "No such member.");
  if (target.role === "owner") throw forbidden("The owner can't leave or be removed.");
  await deps.db.delete(orgMembers).where(and(eq(orgMembers.orgId, orgId), eq(orgMembers.userId, targetId)));
  await audit(deps, orgId, actorId, "member_removed", { member: targetId });
  await deps.events.publish(targetId, "entitlement");
}

export async function setPolicy(deps: Deps, orgId: string, actorId: string, policy: OrgPolicy) {
  await roleIn(deps, orgId, actorId, "admin");
  const before = await policyOf(deps, orgId);
  await deps.db
    .insert(orgPolicies)
    .values({ orgId, policy, updatedAt: deps.clock.now() })
    .onConflictDoUpdate({ target: orgPolicies.orgId, set: { policy, updatedAt: deps.clock.now() } });
  const changed = (Object.keys(policy) as Array<keyof OrgPolicy>).filter(
    (k) => JSON.stringify(before[k]) !== JSON.stringify(policy[k]),
  );
  await audit(deps, orgId, actorId, "policy_changed", { changed: changed.join(",") });
  await notifyOrg(deps, orgId);
  return policy;
}

/** Sets the seat count; the subscription's quantity follows (F-BL-06 AC3). */
export async function setSeats(deps: Deps, orgId: string, actorId: string, seats: number) {
  await roleIn(deps, orgId, actorId, "admin");
  const used = await seatsInUse(deps, orgId);
  if (seats < used) {
    throw new ApiError(
      409,
      "SEATS_IN_USE",
      "Conflict",
      `${used} seats are in use. Remove members or invites first.`,
      {
        used,
      },
    );
  }
  const sub = await orgSubscription(deps, orgId);
  if (!sub || sub.status === "canceled") {
    throw new ApiError(409, "NO_SUBSCRIPTION", "Conflict", "Start the Team plan before changing seats.");
  }
  await deps.billing.updateQuantity(sub.stripeSubscriptionId, seats);
  await deps.db
    .update(subscriptions)
    .set({ seats, updatedAt: deps.clock.now() })
    .where(eq(subscriptions.id, sub.id));
  await audit(deps, orgId, actorId, "seats_changed", { from: sub.seats, to: seats });
  return { seats };
}

export async function auditLog(deps: Deps, orgId: string, viewerId: string, before?: Date, limit = 50) {
  await roleIn(deps, orgId, viewerId, "admin");
  const rows = await deps.db
    .select({
      id: auditEvents.id,
      event: auditEvents.event,
      meta: auditEvents.meta,
      createdAt: auditEvents.createdAt,
      actorEmail: user.email,
    })
    .from(auditEvents)
    .leftJoin(user, eq(user.id, auditEvents.actorUserId))
    .where(and(eq(auditEvents.orgId, orgId), before ? lt(auditEvents.createdAt, before) : undefined))
    .orderBy(desc(auditEvents.createdAt))
    .limit(limit);
  return {
    events: rows.map((r) => ({ ...r, createdAt: r.createdAt.toISOString() })),
    next: rows.length === limit ? (rows.at(-1)?.createdAt.toISOString() ?? null) : null,
  };
}

export const clientAuditBody = z.object({
  events: z
    .array(
      z.object({
        event: z.string().max(60),
        // Metadata only: a few short scalar fields, never text a user wrote.
        meta: z
          .record(z.string().max(40), z.union([z.string().max(100), z.number(), z.boolean(), z.null()]))
          .refine((m) => Object.keys(m).length <= 10, "At most 10 metadata fields.")
          .default({}),
        // Any RFC 3339 time, with Z or an offset.
        ts: z.string().datetime({ offset: true }),
      }),
    )
    .max(100),
});

/**
 * Members' apps report Full-access use and plugin approvals (F-TL-10). Kept
 * only for a Team member whose paid org turned audit on; dropped otherwise.
 */
export async function recordClientAudit(
  deps: Deps,
  userId: string,
  events: z.infer<typeof clientAuditBody>["events"],
): Promise<number> {
  const m = await membershipOf(deps.db, userId);
  if (!m) return 0;
  const { effective } = await orgPlan(deps, m.orgId);
  if (effective.plan !== "team" || !(await policyOf(deps, m.orgId)).auditEnabled) return 0;
  const allowed = new Set<string>(CLIENT_AUDIT_EVENTS);
  const kept = events.filter((e): e is typeof e & { event: ClientAuditEvent } => allowed.has(e.event));
  for (const e of kept) {
    await deps.db.insert(auditEvents).values({
      id: newId("aud"),
      orgId: m.orgId,
      actorUserId: userId,
      event: e.event,
      meta: { ...e.meta, at: e.ts },
    });
  }
  return kept.length;
}
