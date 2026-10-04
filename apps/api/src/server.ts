import { serve } from "@hono/node-server";
import { createApp } from "./app";
import { buildDeps } from "./buildDeps";
import { loadEnv } from "./env";
import { logger } from "./lib/logger";

const env = loadEnv();
const { deps, close } = buildDeps(env);
const app = createApp(deps);
const server = serve({ fetch: app.fetch, port: env.PORT, hostname: "0.0.0.0" }, (info) => {
  logger.info({ port: info.port, billing: deps.billing.kind }, "api listening");
});

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    logger.info({ signal }, "shutting down");
    server.close(() => {
      void close().finally(() => process.exit(0));
    });
    setTimeout(() => process.exit(0), 5000).unref();
  });
}
