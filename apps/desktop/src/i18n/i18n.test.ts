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

  it("formats plurals", () => {
    expect(t("splash.crash.body", { m: 1 })).toBe("We saved 1 session from your last run. Restore them?");
    expect(t("splash.failed", { n: 2 })).toBe("Couldn't restore 2 sessions");
  });
});
