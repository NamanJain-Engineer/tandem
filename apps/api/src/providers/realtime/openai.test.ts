import { describe, expect, it } from "vitest";
import { loadEnv } from "../../env";
import { selectRealtime } from "./index";
import { MockRealtime } from "./mock";
import { OpenAiRealtime } from "./openai";
import { RealtimeUnavailableError } from "./types";

type Seen = { url: string; headers: Headers; body: Record<string, unknown> };

function fakeFetch(answer: () => Response, seen: Seen[]): typeof fetch {
  return (async (url: string | URL | Request, init?: RequestInit) => {
    seen.push({
      url: String(url),
      headers: new Headers(init?.headers),
      body: JSON.parse(String(init?.body)) as Record<string, unknown>,
    });
    return answer();
  }) as typeof fetch;
}

const secret = () =>
  new Response(
    JSON.stringify({
      value: "ek_test_123",
      expires_at: 1_791_600_000,
      session: { type: "realtime", id: "sess_abc", model: "gpt-realtime" },
    }),
    { status: 200, headers: { "content-type": "application/json" } },
  );

describe("F-BE-08 OpenAiRealtime", () => {
  it("F-BE-08 mints a client secret for a realtime session with the talker's instructions, voice and act tool", async () => {
    const seen: Seen[] = [];
    const provider = new OpenAiRealtime({ apiKey: "sk-test", fetch: fakeFetch(secret, seen) });
    const out = await provider.mint({ instructions: "Be brief.", voice: "cedar", maxSeconds: 1800 });
    expect(out).toEqual({
      token: "ek_test_123",
      wsUrl: "wss://api.openai.com/v1/realtime?model=gpt-realtime",
      providerSessionId: "sess_abc",
    });
    expect(seen).toHaveLength(1);
    expect(seen[0]?.url).toBe("https://api.openai.com/v1/realtime/client_secrets");
    expect(seen[0]?.headers.get("authorization")).toBe("Bearer sk-test");
    expect(seen[0]?.body).toEqual({
      expires_after: { anchor: "created_at", seconds: 600 },
      session: {
        type: "realtime",
        model: "gpt-realtime",
        instructions: "Be brief.",
        audio: { output: { voice: "cedar" } },
        tools: [
          {
            type: "function",
            name: "act",
            description: expect.stringContaining("app"),
            parameters: {
              type: "object",
              properties: { request: { type: "string", description: expect.any(String) } },
              required: ["request"],
              additionalProperties: false,
            },
          },
        ],
        tool_choice: "auto",
      },
    });
    expect(await provider.reportedSeconds("sess_abc")).toBeNull();
  });

  it("F-BE-08 an unknown voice falls back to the default, and the model and base can be set", async () => {
    const seen: Seen[] = [];
    const provider = new OpenAiRealtime({
      apiKey: "k",
      baseUrl: "https://realtime.example/",
      model: "gpt-realtime-mini",
      fetch: fakeFetch(secret, seen),
    });
    const out = await provider.mint({ instructions: "x", voice: "not-a-voice", maxSeconds: 1800 });
    expect(out.wsUrl).toBe("wss://realtime.example/v1/realtime?model=gpt-realtime-mini");
    expect(seen[0]?.url).toBe("https://realtime.example/v1/realtime/client_secrets");
    const session = seen[0]?.body.session as Record<string, unknown>;
    expect(session.audio).toEqual({ output: { voice: "marin" } });
    expect(session.model).toBe("gpt-realtime-mini");
  });

  it("F-BE-08 an error answer carries its status and never the body", async () => {
    const seen: Seen[] = [];
    const provider = new OpenAiRealtime({
      apiKey: "k",
      fetch: fakeFetch(() => new Response("secret-body-from-vendor", { status: 401 }), seen),
    });
    const err = await provider.mint({ instructions: "x", maxSeconds: 1800 }).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(RealtimeUnavailableError);
    expect((err as RealtimeUnavailableError).status).toBe(401);
    expect(String((err as Error).message)).not.toContain("secret-body");
    const empty = new OpenAiRealtime({
      apiKey: "k",
      fetch: fakeFetch(() => new Response(JSON.stringify({ session: {} }), { status: 200 }), seen),
    });
    await expect(empty.mint({ instructions: "x", maxSeconds: 1800 })).rejects.toBeInstanceOf(
      RealtimeUnavailableError,
    );
  });

  it("F-BE-08 REALTIME_PROVIDER picks the provider; openai without a key refuses to start", () => {
    expect(selectRealtime(loadEnv({}))).toBeInstanceOf(MockRealtime);
    expect(selectRealtime(loadEnv({ REALTIME_PROVIDER: "openai", OPENAI_API_KEY: "sk-x" }))).toBeInstanceOf(
      OpenAiRealtime,
    );
    expect(() => selectRealtime(loadEnv({ REALTIME_PROVIDER: "openai" }))).toThrow("OPENAI_API_KEY");
  });
});
