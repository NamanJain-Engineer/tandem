import { describe, expect, it } from "vitest";
import { createApp } from "../src/app";
import { redactHeaders, redactText } from "../src/lib/redact";

describe("api foundations", () => {
  it("GET /v1/health returns ok and version and echoes the request id", async () => {
    const res = await createApp().request("/v1/health", {
      headers: {
        "x-request-id": "req-1",
        "x-tandem-app": "desktop",
        "x-tandem-version": "0.1.0",
        "x-tandem-platform": "linux",
      },
    });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, version: "0.1.0" });
    expect(res.headers.get("x-request-id")).toBe("req-1");
  });

  it("unknown routes return RFC 9457 problem details", async () => {
    const res = await createApp().request("/missing");
    expect(res.status).toBe(404);
    expect(res.headers.get("content-type")).toContain("application/problem+json");
    expect(await res.json()).toMatchObject({
      type: "about:blank",
      title: "Not Found",
      status: 404,
      code: "NOT_FOUND",
    });
  });

  it("rejects an invalid request id and generates one", async () => {
    const res = await createApp().request("/v1/health", {
      headers: { "x-request-id": "bad id with spaces" },
    });
    expect(res.headers.get("x-request-id")).toMatch(/^[0-9a-f-]{36}$/);
  });

  it("redacts vendor tokens and sensitive headers", () => {
    const secret = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
    expect(redactText(`token ${secret}`)).toBe("token [REDACTED]");
    const cleaned = redactHeaders(
      new Headers({ authorization: "Bearer x", cookie: "s=1", "x-tandem-app": "desktop" }),
    );
    expect(cleaned).toMatchObject({
      authorization: "[REDACTED]",
      cookie: "[REDACTED]",
      "x-tandem-app": "desktop",
    });
  });
});
