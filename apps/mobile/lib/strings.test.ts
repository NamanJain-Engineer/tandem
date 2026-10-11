import { readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { EN, t } from "./strings";

describe("F-RM-05 strings", () => {
  it("fills placeholders", () => {
    expect(t("type.title", { agent: "auth tests" })).toBe("Type to auth tests");
  });

  it("every t() key the screens use exists in the catalog", () => {
    const files = ["../App.tsx", ...readdirSync(resolve(import.meta.dirname, "../screens")).map((f) => `../screens/${f}`)];
    for (const f of files) {
      const src = readFileSync(resolve(import.meta.dirname, f), "utf8");
      for (const m of src.matchAll(/\bt\("([\w.]+)"/g)) expect(EN, `${f}: ${m[1]}`).toHaveProperty([m[1]!]);
    }
  });

  it("names no other product", () => {
    const text = Object.values(EN).join(" ");
    expect(text).not.toMatch(/bridge/i);
  });
});
