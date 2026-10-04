import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { VERSION } from "../routes/health";
import { buildOpenApi } from "./registry";

describe("openapi.json", () => {
  it("is generated from the zod routes and up to date (run `pnpm --filter api openapi`)", () => {
    const committed = JSON.parse(readFileSync(new URL("../../openapi.json", import.meta.url), "utf8"));
    expect(committed).toEqual(buildOpenApi(VERSION));
  });
});
