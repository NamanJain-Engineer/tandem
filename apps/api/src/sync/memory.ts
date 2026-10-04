import type { SyncEvent, SyncPage } from "@tandem/protocol";
import type { NewSyncEvent, SyncRepo } from "./types";

export class MemorySyncRepo implements SyncRepo {
  private seq = 0;
  private readonly byId = new Map<string, SyncEvent>();
  private readonly ordered: SyncEvent[] = [];

  async append(events: NewSyncEvent[]): Promise<SyncEvent[]> {
    const stored: SyncEvent[] = [];
    for (const event of events) {
      const existing = this.byId.get(event.id);
      if (existing) {
        stored.push(existing);
        continue;
      }
      this.seq += 1;
      const row: SyncEvent = { ...event, seq: this.seq };
      this.byId.set(event.id, row);
      this.ordered.push(row);
      stored.push(row);
    }
    return stored;
  }

  async list(workspaceId: string, after: number, limit: number): Promise<SyncPage> {
    const matched = this.ordered.filter(
      (event) => event.workspaceId === workspaceId && event.seq > after,
    );
    const items = matched.slice(0, limit);
    const nextCursor = matched.length > limit ? (items.at(-1)?.seq ?? null) : null;
    return { items, nextCursor };
  }
}
