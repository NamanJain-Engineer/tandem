import { serve } from "@hono/node-server";
import { createApp } from "./app";
import { buildDeps } from "./buildDeps";
import { loadEnv } from "./env";
import { startDeletionJob } from "./jobs/deletionJob";
import { startReservationSweeper } from "./jobs/reservationSweeper";
import { startVoiceSweeper } from "./jobs/voiceSweeper";
import { logger } from "./lib/logger";
import { Relay } from "./routes/relay";
import { relayAuthenticator } from "./services/pairings";

const env = loadEnv();
const { deps, close } = buildDeps(env);
const app = createApp(deps);
const stopSweeper = startReservationSweeper(deps);
const stopDeletion = startDeletionJob(deps);
const stopVoiceSweeper = startVoiceSweeper(deps);
const server = serve({ fetch: app.fetch, port: env.PORT, hostname: "0.0.0.0" }, (info) => {
  logger.info({ port: info.port, billing: deps.billing.kind }, "api listening");
});
// The phone relay shares the API's port (F-BE-11).
const relay = new Relay({ authenticate: relayAuthenticator(deps), control: deps.relay, now: () => Date.now() });
relay.attach(server);

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    logger.info({ signal }, "shutting down");
    stopSweeper();
    relay.close();
    stopDeletion();
    stopVoiceSweeper();
    server.close(() => {
      void close().finally(() => process.exit(0));
    });
    setTimeout(() => process.exit(0), 5000).unref();
  });
}
