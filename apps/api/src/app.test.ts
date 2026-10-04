import { describe, expect, it } from "vitest";
import { createApp } from "./app";
import { redactText } from "./redact";

describe("api", () => {
  it("GET /v1/health returns ok and version", async () => {
    const app = createApp();
    const res = await app.request("/v1/health", {
      headers: {
        "x-request-id": "req-1",
        "x-tandem-app": "desktop",
        "x-tandem-version": "0.1.0",
        "x-tandem-platform": "linux",
        authorization: "Bearer ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij",
      },
    });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ok: true, version: "0.1.0" });
    expect(res.headers.get("x-request-id")).toBe("req-1");
  });

  it("unknown routes are problem details", async () => {
    const app = createApp();
    const res = await app.request("/missing");
    expect(res.status).toBe(404);
    const body = await res.json();
    expect(body.status).toBe(404);
    expect(body.title).toBe("Not Found");
    expect(body.type).toBe("about:blank");
  });

  it("redacts vendor tokens", () => {
    const secret = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
    const cleaned = redactText(`token ${secret}`);
    expect(cleaned).not.toContain(secret);
    expect(cleaned).toContain("[REDACTED]");
  });
});
