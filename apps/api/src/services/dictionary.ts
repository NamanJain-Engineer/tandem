import { isDeepStrictEqual } from "node:util";
import { and, eq, gt, sql } from "drizzle-orm";
import { z } from "zod";
import type { Deps } from "../deps";
import { dictionarySync } from "../db/schema";

/** One rule change as a device sends it (ARCHITECTURE §13, F-DI-09). */
const change = z.object({
  rule_id: z.string().min(1).max(200),
  payload: z.record(z.string(), z.unknown()),
  deleted: z.boolean(),
  updated_at: z.string().datetime({ offset: true }),
});

export const syncBody = z.object({
  since_rev: z.number().int().min(0),
  changes: z.array(change).max(500),
});

type Change = z.infer<typeof change>;
type Row = typeof dictionarySync.$inferSelect;
export type SyncedChange = Change & { rev: number };
export type SyncResponse = {
  rev: number;
  changes: SyncedChange[];
  conflicts: Array<{ rule_id: string; local: Change; remote: SyncedChange }>;
};

/** Transcript evidence stays on the device that heard it. */
function withoutEvidence(payload: Record<string, unknown>): Record<string, unknown> {
  const { evidence: _evidence, ...rest } = payload;
  return rest;
}

function toChange(row: Row): SyncedChange {
  return {
    rule_id: row.ruleId,
    payload: row.payload as Record<string, unknown>,
    deleted: row.deleted,
    updated_at: row.updatedAt.toISOString(),
    rev: row.rev,
  };
}

/**
 * Last writer wins per rule by `updated_at`. A change older than the stored
 * one is not written: when it differs, the pair comes back as a conflict for
 * review, on every sync until the device sends a newer write or stops
 * sending it. Each
 * write takes the user's next rev, so a device pulls everything after the
 * rev it last saw.
 */
export async function syncDictionary(deps: Deps, userId: string, body: z.infer<typeof syncBody>): Promise<SyncResponse> {
  return deps.db.transaction(async (tx) => {
    // One sync per user at a time keeps the rev counter gapless and ordered.
    await tx.execute(sql`select pg_advisory_xact_lock(hashtext(${`dictionary:${userId}`}))`);
    const [top] = await tx
      .select({ rev: sql<number>`coalesce(max(${dictionarySync.rev}), 0)::bigint` })
      .from(dictionarySync)
      .where(eq(dictionarySync.userId, userId));
    let rev = Number(top?.rev ?? 0);
    const written = new Set<string>();
    const conflicts: SyncResponse["conflicts"] = [];
    for (const c of body.changes) {
      const [stored] = await tx
        .select()
        .from(dictionarySync)
        .where(and(eq(dictionarySync.userId, userId), eq(dictionarySync.ruleId, c.rule_id)));
      const updatedAt = new Date(c.updated_at);
      if (stored && updatedAt < stored.updatedAt) {
        const payload = withoutEvidence(c.payload);
        const same = stored.deleted === c.deleted && isDeepStrictEqual(stored.payload, payload);
        if (!same) conflicts.push({ rule_id: c.rule_id, local: { ...c, payload }, remote: toChange(stored) });
        continue;
      }
      rev += 1;
      const values = { payload: withoutEvidence(c.payload), deleted: c.deleted, updatedAt, rev };
      await tx
        .insert(dictionarySync)
        .values({ userId, ruleId: c.rule_id, ...values })
        .onConflictDoUpdate({ target: [dictionarySync.userId, dictionarySync.ruleId], set: values });
      written.add(c.rule_id);
    }
    const newer = await tx
      .select()
      .from(dictionarySync)
      .where(and(eq(dictionarySync.userId, userId), gt(dictionarySync.rev, body.since_rev)))
      .orderBy(dictionarySync.rev);
    return { rev, changes: newer.filter((r) => !written.has(r.ruleId)).map(toChange), conflicts };
  });
}

/** Forgets the user's synced copy (sync turned off). Allowed on any plan. */
export async function deleteSyncedDictionary(deps: Deps, userId: string): Promise<void> {
  await deps.db.delete(dictionarySync).where(eq(dictionarySync.userId, userId));
}
