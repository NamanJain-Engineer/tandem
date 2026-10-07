// `pnpm --filter api openapi`: writes apps/api/openapi.json from the route registry.
import { writeFileSync } from "node:fs";
import { buildOpenApi } from "../src/openapi/registry";
import { VERSION } from "../src/routes/health";

writeFileSync(
  new URL("../openapi.json", import.meta.url),
  JSON.stringify(buildOpenApi(VERSION), null, 2) + "\n",
);
console.log("openapi.json written");
