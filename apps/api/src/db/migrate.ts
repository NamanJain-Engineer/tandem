// `pnpm --filter api db:migrate`: applies drizzle/ migrations.
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { fileURLToPath } from "node:url";
import { createDb } from "./client";

export async function runMigrations(url: string): Promise<void> {
  const { db, pool } = createDb(url);
  try {
    await migrate(db, { migrationsFolder: fileURLToPath(new URL("../../drizzle", import.meta.url)) });
  } finally {
    await pool.end();
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const url = process.env.DATABASE_URL ?? "postgres://tandem:tandem@localhost:5432/tandem";
  runMigrations(url)
    .then(() => console.log("migrations applied"))
    .catch((err: unknown) => {
      console.error(err);
      process.exit(1);
    });
}
