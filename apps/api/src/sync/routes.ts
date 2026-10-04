import { Hono } from "hono";
import { z } from "zod";
import { problem } from "../problem";
import type { SyncRepo } from "./types";

const eventSchema = z.object({
  id: z.string().uuid(),
  workspaceId: z.string().min(1).max(80),
  kind: z.string().regex(/^[a-z0-9._-]{1,64}$/),
  payload: z.record(z.string(), z.unknown()).default({}),
  createdAt: z.string().datetime({ offset: true }),
});

const batchSchema = z.object({
  events: z.array(eventSchema).min(1).max(100),
});

const listSchema = z.object({
  workspaceId: z.string().min(1).max(80),
  after: z.coerce.number().int().min(0).default(0),
  limit: z.coerce.number().int().min(1).max(100).default(50),
});

export function syncRoutes(repo: SyncRepo) {
  const routes = new Hono();

  routes.post("/", async (c) => {
    const body: unknown = await c.req.json().catch(() => null);
    const parsed = batchSchema.safeParse(body);
    if (!parsed.success) {
      return c.json(problem(400, "Bad Request", parsed.error.issues[0]?.message ?? "Invalid body"), 400);
    }
    const items = await repo.append(parsed.data.events);
    return c.json({ items });
  });

  routes.get("/", async (c) => {
    const raw: Record<string, string> = {};
    for (const key of ["workspaceId", "after", "limit"] as const) {
      const value = c.req.query(key);
      if (value !== undefined) raw[key] = value;
    }
    const parsed = listSchema.safeParse(raw);
    if (!parsed.success) {
      return c.json(problem(400, "Bad Request", parsed.error.issues[0]?.message ?? "Invalid query"), 400);
    }
    const page = await repo.list(parsed.data.workspaceId, parsed.data.after, parsed.data.limit);
    return c.json(page);
  });

  return routes;
}
