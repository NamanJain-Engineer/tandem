import { describe, expect, it } from "vitest";
import { callerCountry, FREE_GEMINI_EXCLUDED } from "./region";

describe("F-BE-08 where Google's free tier may not serve (D-1018)", () => {
  it("F-BE-08 is the EEA, the EU's regions with codes of their own, Switzerland and the UK", () => {
    const eu = "AT BE BG HR CY CZ DK EE FI FR DE GR HU IE IT LV LT LU MT NL PL PT RO SK SI ES SE".split(" ");
    expect(eu).toHaveLength(27);
    for (const c of [...eu, "IS", "LI", "NO", "CH", "GB"]) expect(FREE_GEMINI_EXCLUDED.has(c), c).toBe(true);
    // Åland and the French outermost regions are the EU, under codes of their own.
    for (const c of ["AX", "GF", "GP", "MQ", "RE", "YT", "MF"])
      expect(FREE_GEMINI_EXCLUDED.has(c), c).toBe(true);
    // Names some databases use: UK, Greece's EL, and EU for "somewhere in Europe".
    for (const c of ["UK", "EL", "EU"]) expect(FREE_GEMINI_EXCLUDED.has(c), c).toBe(true);
    for (const c of ["US", "CA", "IN", "JP", "BR", "TR", "UA", "RS", "AL", "MC"]) {
      expect(FREE_GEMINI_EXCLUDED.has(c), c).toBe(false);
    }
  });

  it("F-BE-08 a header is a country only as two letters; XX, ZZ and Tor's T1 are unknown", () => {
    expect(callerCountry(" gb ")).toBe("GB");
    expect(callerCountry("US")).toBe("US");
    for (const v of [undefined, "", "XX", "ZZ", "T1", "A1", "USA", "g b"])
      expect(callerCountry(v), String(v)).toBeNull();
  });
});
