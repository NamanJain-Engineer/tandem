export type Health = {
  ok: true;
  version: string;
};

export type Problem = {
  type: string;
  title: string;
  status: number;
  detail?: string;
};

export type SyncEvent = {
  seq: number;
  id: string;
  workspaceId: string;
  kind: string;
  payload: Record<string, unknown>;
  createdAt: string;
};

export type SyncPage = {
  items: SyncEvent[];
  nextCursor: number | null;
};

