// Where a caller is, for the one rule that needs it (D-1018): Google's terms
// allow only paid Gemini services for users in the EEA, Switzerland and the UK
// (Gemini API Additional Terms), so free-tier calls from there go elsewhere.

const EU = "AT BE BG HR CY CZ DK EE FI FR DE GR HU IE IT LV LT LU MT NL PL PT RO SK SI ES SE";

/**
 * The EEA (the EU's 27, Iceland, Liechtenstein, Norway), the EU's regions
 * that have codes of their own (Åland and the French outermost regions),
 * Switzerland and the UK, and the names some databases use for them (UK,
 * Greece's EL, and EU for "somewhere in Europe").
 */
export const FREE_GEMINI_EXCLUDED: ReadonlySet<string> = new Set([
  ...EU.split(" "),
  ..."IS LI NO".split(" "),
  ..."AX GF GP MQ RE YT MF".split(" "),
  "CH",
  "GB",
  ..."UK EL EU".split(" "),
]);

/**
 * The caller's country from the edge's header (ISO 3166-1 alpha-2), or null
 * when it isn't one: missing, XX or ZZ (unknown), or Tor's T1.
 */
export function callerCountry(header: string | undefined): string | null {
  const code = header?.trim().toUpperCase() ?? "";
  if (!/^[A-Z]{2}$/.test(code) || code === "XX" || code === "ZZ") return null;
  return code;
}
