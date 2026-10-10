import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { CREDIT_PRICES } from "@tandem/protocol/plans";
import { ledgerLabel, PRICE_LIST } from "./credits";

describe("F-WB-05 credits page copy", () => {
  it("names each ledger row in words", () => {
    const row = { id: "1", bucket: "monthly" as const, delta: -3, reason: "reserve", feature: "rewrite", createdAt: "" };
    expect(ledgerLabel(row)).toBe("Used · Enhance or Polish");
    expect(ledgerLabel({ ...row, reason: "pack_purchase", feature: null })).toBe("Credit pack");
  });

  it("takes every price from plans.ts", () => {
    expect(PRICE_LIST.find((p) => p.id === "F-DI-05")?.price).toContain(String(CREDIT_PRICES.stt_15s));
    expect(PRICE_LIST.find((p) => p.id === "F-DI-10")?.price).toContain(String(CREDIT_PRICES.rewrite));
    expect(PRICE_LIST.find((p) => p.id === "F-AG-04")?.price).toContain(String(CREDIT_PRICES.teammate_draft));
  });

  it("prices only features the app has: each row's feature is ticked in REMAINING", () => {
    const remaining = readFileSync(join(import.meta.dirname, "../../../docs/REMAINING.md"), "utf8");
    for (const p of PRICE_LIST) expect(remaining, p.id).toContain(`- [x] ${p.id} `);
    // Cloud dictation and Enhance or Polish shipped with M8's Dictate (D-824).
    expect(PRICE_LIST.map((p) => p.id)).toEqual(expect.arrayContaining(["F-DI-05", "F-DI-10"]));
  });
});
