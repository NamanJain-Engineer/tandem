import type { SyncEvent, SyncPage } from "@tandem/protocol";

export type NewSyncEvent = Omit<SyncEvent, "seq">;

export interface SyncRepo {
  append(events: NewSyncEvent[]): Promise<SyncEvent[]>;
  list(workspaceId: string, after: number, limit: number): Promise<SyncPage>;
}
