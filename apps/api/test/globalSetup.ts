// Creates a fresh `tandem_test` database (compose Postgres) and migrates it.
// API tests need `pnpm infra:up` (Postgres + Redis); see BUILD_LOG D-20.
import pg from "pg";
import { runMigrations } from "../src/db/migrate";

export const TEST_DB = "tandem_test";
const ADMIN_URL = process.env.TEST_ADMIN_DATABASE_URL ?? "postgres://tandem:tandem@localhost:5432/tandem";

export async function setup(): Promise<void> {
  const admin = new pg.Client({ connectionString: ADMIN_URL });
  await admin.connect();
  await admin.query(`DROP DATABASE IF EXISTS ${TEST_DB} WITH (FORCE)`);
  await admin.query(`CREATE DATABASE ${TEST_DB}`);
  await admin.end();
  await runMigrations(ADMIN_URL.replace(/\/[^/]+$/, `/${TEST_DB}`));
}
