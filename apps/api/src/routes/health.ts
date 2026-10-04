import { Hono } from "hono";
import type { Health } from "@tandem/protocol";

export const VERSION = "0.1.0";

export const health = new Hono().get("/", (c) => {
  const body: Health = { ok: true, version: VERSION };
  return c.json(body);
});
