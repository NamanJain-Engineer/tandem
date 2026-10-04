import { pgTable, text, timestamp } from "drizzle-orm/pg-core";

/** Placeholder table so Drizzle has a schema. Account tables land in M1. */
export const schemaMigrations = pgTable("schema_migrations", {
  id: text("id").primaryKey(),
  appliedAt: timestamp("applied_at", { withTimezone: true }).notNull(),
});
