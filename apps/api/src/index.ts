import { serve } from "@hono/node-server";
import { createApp } from "./app";
import { logger } from "./log";
import { openSyncRepo } from "./sync/repo";

const port = Number(process.env.PORT ?? 8787);
const sync = await openSyncRepo();
const app = createApp({ sync });

serve({ fetch: app.fetch, port }, (info) => {
  logger.info({ port: info.port }, "api listening");
});
