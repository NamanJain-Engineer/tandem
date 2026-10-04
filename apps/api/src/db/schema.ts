import { bigint, jsonb, pgTable, text, timestamp } from "drizzle-orm/pg-core";

export const schemaMigrations = pgTable("schema_migrations", {
  id: text("id").primaryKey(),
  appliedAt: timestamp("applied_at", { withTimezone: true }).notNull(),
});

export const syncEvents = pgTable("sync_events", {
  seq: bigint("seq", { mode: "number" }).primaryKey().generatedAlwaysAsIdentity(),
  id: text("id").notNull().unique(),
  workspaceId: text("workspace_id").notNull(),
  kind: text("kind").notNull(),
  payload: jsonb("payload").$type<Record<string, unknown>>().notNull(),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull(),
});

