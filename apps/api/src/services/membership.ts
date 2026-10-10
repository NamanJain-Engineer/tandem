// Who pays for a user's hosted calls (ARCHITECTURE §14.5): a Team member
// spends from the org's pool, everyone else from their own buckets.
import { eq } from "drizzle-orm";
import type { Db } from "../db/client";
import { orgMembers } from "../db/schema";
import type { Owner } from "./ledger";

export type OrgRole = "owner" | "admin" | "member";
export type Membership = { orgId: string; role: OrgRole; monthlyCap: number | null };

type Reader = Pick<Db, "select">;

export async function membershipOf(db: Reader, userId: string): Promise<Membership | null> {
  const [row] = await db.select().from(orgMembers).where(eq(orgMembers.userId, userId)).limit(1);
  return row ? { orgId: row.orgId, role: row.role, monthlyCap: row.monthlyCap } : null;
}

/** The credit owner for `userId`: their org when they belong to one, else themselves. */
export async function creditOwnerOf(db: Reader, userId: string): Promise<Owner> {
  const m = await membershipOf(db, userId);
  return m ? { kind: "org", id: m.orgId } : { kind: "user", id: userId };
}

/** Everyone whose balance view changes when `owner`'s buckets change. */
export async function usersOf(db: Reader, owner: Owner): Promise<string[]> {
  if (owner.kind === "user") return [owner.id];
  const rows = await db
    .select({ userId: orgMembers.userId })
    .from(orgMembers)
    .where(eq(orgMembers.orgId, owner.id));
  return rows.map((r) => r.userId);
}
