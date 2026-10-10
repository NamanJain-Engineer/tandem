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
    expect(PRICE_LIST.find((p) => p.feature === "Cloud dictation")?.price).toContain(String(CREDIT_PRICES.stt_15s));
    expect(PRICE_LIST.find((p) => p.feature === "Enhance or Polish")?.price).toContain(String(CREDIT_PRICES.rewrite));
  });
});
