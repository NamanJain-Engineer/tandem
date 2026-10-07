import { describe, expect, it } from "vitest";
import { ApiError, createClient } from "./client";

describe("api client", () => {
  it("sends app headers and bearer token, parses problems", async () => {
    const seen: Request[] = [];
    const fakeFetch = async (input: RequestInfo | URL, init?: RequestInit) => {
      const req = new Request(input, init);
      seen.push(req);
      if (req.url.endsWith("/v1/health")) {
        return new Response(JSON.stringify({ ok: true, version: "0.1.0" }), { status: 200 });
      }
      return new Response(
        JSON.stringify({ type: "about:blank", title: "Unauthorized", status: 401, code: "UNAUTHENTICATED" }),
        {
          status: 401,
        },
      );
    };
    const client = createClient({
      baseUrl: "http://127.0.0.1:8787",
      token: () => "t0k",
      app: { name: "desktop", version: "0.1.0", platform: "linux" },
      fetch: fakeFetch as typeof fetch,
    });
    await expect(client.health()).resolves.toEqual({ ok: true, version: "0.1.0" });
    expect(seen[0]?.headers.get("authorization")).toBe("Bearer t0k");
    expect(seen[0]?.headers.get("x-tandem-platform")).toBe("linux");
    await expect(client.request("GET", "/v1/me")).rejects.toBeInstanceOf(ApiError);
  });
});
