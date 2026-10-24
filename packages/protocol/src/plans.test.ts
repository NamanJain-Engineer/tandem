import { describe, expect, it } from "vitest";
import { CREDIT_PACKS, CREDIT_PRICES, LIMITS, PRICES } from "./plans";

describe("plans", () => {
  it("matches the §3 tables", () => {
    expect(PRICES.pro).toMatchObject({ month: 2900, year: 29000, trialDays: 7 });
    expect(PRICES.team).toMatchObject({ month: 4900, year: 49000, trialDays: 14, minSeats: 2 });
    expect(CREDIT_PACKS["12k"]).toEqual({ credits: 12_000, priceCents: 1000 });
    expect(CREDIT_PRICES).toMatchObject({
      stt_15s: 2,
      rewrite: 3,
      voice_10s: 20,
      auto_turn: 6,
      teammate_draft: 10,
    });
  });
});

import freeLimits from "../fixtures/free-limits.json";

describe("free limits fixture", () => {
  it("is the same object the Rust client falls back to", () => {
    expect(LIMITS.free).toEqual(freeLimits);
  });

  it("F-DI-09 dictionary sync is hosted: off with no account", () => {
    expect(LIMITS.free.dictionarySync).toBe(false);
    expect(LIMITS.pro.dictionarySync).toBe(true);
    expect(LIMITS.team.dictionarySync).toBe(true);
  });

  it("F-CD-03 every local feature is free and unlimited with no account (D-869, D-2900)", () => {
    expect(LIMITS.free).toMatchObject({
      panes: null,
      workspaces: null,
      threadGrid: 6,
      teammates: null,
      routines: true,
      backgroundRoutines: true,
      taskExecution: true,
      pluginConnections: null,
      accountsPerEngine: null,
      chatEngines: null,
      sessionSearch: true,
    });
  });

  it("F-CD-03 hosted features stay off until accounts open (D-2900)", () => {
    const { cloudStt, enhance, conductor, auto, phone, admin, creditPacks, dictionarySync } = LIMITS.free;
    expect({ cloudStt, enhance, conductor, auto, phone, admin, creditPacks, dictionarySync }).toEqual({
      cloudStt: false,
      enhance: false,
      conductor: false,
      auto: false,
      phone: false,
      admin: false,
      creditPacks: false,
      dictionarySync: false,
    });
  });

  it("F-CD-03 the account plans never allow less than no account does (D-2900)", () => {
    for (const plan of ["pro", "team"] as const) {
      for (const [key, free] of Object.entries(LIMITS.free) as [keyof typeof LIMITS.free, unknown][]) {
        const paid = LIMITS[plan][key];
        if (free === null) expect(paid, `${plan}.${key}`).toBeNull();
        else if (free === true) expect(paid, `${plan}.${key}`).toBe(true);
        else if (typeof free === "number")
          expect(paid as number, `${plan}.${key}`).toBeGreaterThanOrEqual(free);
      }
    }
  });
});
