import { MemorySyncRepo } from "./memory";
import { PgSyncRepo } from "./pg";
import type { SyncRepo } from "./types";

export async function openSyncRepo(): Promise<SyncRepo> {
  const databaseUrl = process.env.DATABASE_URL;
  if (!databaseUrl) {
    return new MemorySyncRepo();
  }
  return PgSyncRepo.open(databaseUrl);
}
