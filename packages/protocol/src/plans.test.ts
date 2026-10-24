import { describe, expect, it } from "vitest";
import { CREDIT_PACKS, CREDIT_PRICES, LIMITS, PRICES } from "./plans";

describe("plans", () => {
  it("matches the §3 tables", () => {
    expect(PRICES.pro).toMatchObject({ month: 2900, year: 29000, trialDays: 7 });
    expect(PRICES.team).toMatchObject({ month: 4900, year: 49000, trialDays: 14, minSeats: 2 });
    expect(LIMITS.free).toMatchObject({
      panes: 4,
      workspaces: 3,
      teammates: 1,
      pluginConnections: 1,
      routines: false,
    });
    expect(LIMITS.free.chatEngines).toEqual(["claude", "codex"]);
    expect(LIMITS.pro.panes).toBe(64);
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

  it("F-DI-09 dictionary sync is Pro and Team", () => {
    expect(LIMITS.free.dictionarySync).toBe(false);
    expect(LIMITS.pro.dictionarySync).toBe(true);
    expect(LIMITS.team.dictionarySync).toBe(true);
    expect(LIMITS.free.sessionSearch).toBe(false);
    expect(LIMITS.pro.sessionSearch).toBe(true);
    expect(LIMITS.team.sessionSearch).toBe(true);
  });
});
