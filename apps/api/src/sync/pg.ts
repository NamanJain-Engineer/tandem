import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { and, asc, eq, gt } from "drizzle-orm";
import { drizzle } from "drizzle-orm/postgres-js";
import postgres from "postgres";
import type { SyncEvent, SyncPage } from "@tandem/protocol";
import { syncEvents } from "../db/schema";
import type { NewSyncEvent, SyncRepo } from "./types";

const migrationSql = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "../../migrations/0001_sync_events.sql"),
  "utf8",
);

export class PgSyncRepo implements SyncRepo {
  private constructor(private readonly db: ReturnType<typeof drizzle>) {}

  static async open(databaseUrl: string): Promise<PgSyncRepo> {
    const sql = postgres(databaseUrl, { max: 10 });
    await sql.unsafe(migrationSql);
    return new PgSyncRepo(drizzle(sql));
  }

  async append(events: NewSyncEvent[]): Promise<SyncEvent[]> {
    const stored: SyncEvent[] = [];
    for (const event of events) {
      const inserted = await this.db
        .insert(syncEvents)
        .values({
          id: event.id,
          workspaceId: event.workspaceId,
          kind: event.kind,
          payload: event.payload,
          createdAt: new Date(event.createdAt),
        })
        .onConflictDoNothing({ target: syncEvents.id })
        .returning();
      const row = inserted[0];
      if (row) {
        stored.push(toEvent(row));
        continue;
      }
      const [existing] = await this.db
        .select()
        .from(syncEvents)
        .where(eq(syncEvents.id, event.id))
        .limit(1);
      if (!existing) {
        throw new Error(`sync event ${event.id} was not stored`);
      }
      stored.push(toEvent(existing));
    }
    return stored;
  }

  async list(workspaceId: string, after: number, limit: number): Promise<SyncPage> {
    const rows = await this.db
      .select()
      .from(syncEvents)
      .where(and(eq(syncEvents.workspaceId, workspaceId), gt(syncEvents.seq, after)))
      .orderBy(asc(syncEvents.seq))
      .limit(limit + 1);
    const page = rows.slice(0, limit).map(toEvent);
    const nextCursor = rows.length > limit ? (page.at(-1)?.seq ?? null) : null;
    return { items: page, nextCursor };
  }
}

function toEvent(row: typeof syncEvents.$inferSelect): SyncEvent {
  return {
    seq: row.seq,
    id: row.id,
    workspaceId: row.workspaceId,
    kind: row.kind,
    payload: row.payload,
    createdAt: row.createdAt.toISOString(),
  };
}
