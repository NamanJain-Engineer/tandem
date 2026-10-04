import { serve } from "@hono/node-server";
import { createApp } from "./app";
import { logger } from "./log";

const port = Number(process.env.PORT ?? 8787);
const app = createApp();

serve({ fetch: app.fetch, port }, (info) => {
  logger.info({ port: info.port }, "api listening");
});
