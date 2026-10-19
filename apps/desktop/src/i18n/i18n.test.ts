import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import en from "./en.json";
import { t } from "./index";
import { IntlMessageFormat } from "intl-messageformat";

describe("i18n catalog", () => {
  it("every message parses as ICU MessageFormat", () => {
    for (const [key, message] of Object.entries(en)) {
      expect(() => new IntlMessageFormat(message, "en"), key).not.toThrow();
    }
  });

  // A key written twice parses, and the last one silently wins (D-866).
  it("each key is in the catalog once", () => {
    const raw = readFileSync(join(import.meta.dirname, "en.json"), "utf8");
    const keys = [...raw.matchAll(/^\s*"((?:[^"\\]|\\.)*)"\s*:/gm)].map((m) => m[1]);
    expect(keys.filter((k, i) => keys.indexOf(k) !== i)).toEqual([]);
  });

  it("formats plurals", () => {
    expect(t("splash.crash.body", { m: 1 })).toBe("We saved 1 session from your last run. Restore them?");
    expect(t("splash.failed", { n: 2 })).toBe("Couldn't restore 2 sessions");
  });
});
