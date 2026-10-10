// `pnpm --filter api openapi`: writes apps/api/openapi.json from the route
// registry, and apps/api/voice-tools.json from the Conductor doer's catalog.
import { writeFileSync } from "node:fs";
import { buildOpenApi } from "../src/openapi/registry";
import { VERSION } from "../src/routes/health";
import { catalogJson } from "../src/services/voiceTools";

writeFileSync(
  new URL("../openapi.json", import.meta.url),
  JSON.stringify(buildOpenApi(VERSION), null, 2) + "\n",
);
writeFileSync(new URL("../voice-tools.json", import.meta.url), JSON.stringify(catalogJson(), null, 2) + "\n");
console.log("openapi.json and voice-tools.json written");
