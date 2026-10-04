import { describe, expect, it } from "vitest";
import { redactHeaders } from "./redact";

describe("redactHeaders", () => {
  it("censors authorization and cookies", () => {
    const headers = new Headers({
      authorization: "Bearer secret",
      cookie: "session=abc",
      "x-tandem-app": "desktop",
    });
    const cleaned = redactHeaders(headers);
    expect(cleaned.authorization).toBe("[REDACTED]");
    expect(cleaned.cookie).toBe("[REDACTED]");
    expect(cleaned["x-tandem-app"]).toBe("desktop");
  });
});
