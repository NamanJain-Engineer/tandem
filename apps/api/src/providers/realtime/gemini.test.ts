import { describe, expect, it } from "vitest";
import { loadEnv } from "../../env";
import { GeminiLive } from "./gemini";
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

const authToken = () =>
  new Response(JSON.stringify({ name: "auth_tokens/eph_abc123" }), {
    status: 200,
    headers: { "content-type": "application/json" },
  });

const NOW = Date.parse("2026-10-19T10:00:00.000Z");
const WS =
  "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1alpha.GenerativeService.BidiGenerateContentConstrained";

describe("F-BE-08 GeminiLive (D-1014)", () => {
  it("F-BE-08 mints an ephemeral token that locks the model, spoken answers in the voice, the act tool and transcripts", async () => {
    const seen: Seen[] = [];
    const provider = new GeminiLive({ apiKey: "gk-test", fetch: fakeFetch(authToken, seen), now: () => NOW });
    const out = await provider.mint({ instructions: "Be brief.", voice: "kore", maxSeconds: 1800 });
    // The model rides in the fragment, which never leaves the app.
    expect(out).toEqual({
      token: "auth_tokens/eph_abc123",
      wsUrl: `${WS}#model=models/gemini-3.1-flash-live-preview`,
    });
    expect(seen).toHaveLength(1);
    expect(seen[0]?.url).toBe("https://generativelanguage.googleapis.com/v1alpha/auth_tokens");
    expect(seen[0]?.headers.get("x-goog-api-key")).toBe("gk-test");
    expect(seen[0]?.headers.get("authorization")).toBeNull();
    expect(seen[0]?.body).toEqual({
      uses: 1,
      // The first connection within a minute; resuming it later doesn't count as a use.
      newSessionExpireTime: "2026-10-19T10:01:00.000Z",
      // Messages until the call's cap, plus 5 minutes.
      expireTime: "2026-10-19T10:35:00.000Z",
      bidiGenerateContentSetup: {
        model: "models/gemini-3.1-flash-live-preview",
        generationConfig: {
          responseModalities: ["AUDIO"],
          speechConfig: { voiceConfig: { prebuiltVoiceConfig: { voiceName: "Kore" } } },
        },
        tools: [
          {
            functionDeclarations: [
              {
                name: "act",
                description: expect.stringContaining("app"),
                parameters: {
                  type: "object",
                  properties: { request: { type: "string", description: expect.any(String) } },
                  required: ["request"],
                },
              },
            ],
          },
        ],
        inputAudioTranscription: {},
        outputAudioTranscription: {},
        contextWindowCompression: { slidingWindow: {} },
      },
      fieldMask:
        "model,generationConfig.responseModalities,generationConfig.speechConfig,tools,inputAudioTranscription,outputAudioTranscription,contextWindowCompression",
    });
    // The app sends its own instructions and session resumption handle, so neither is in the token.
    expect(JSON.stringify(seen[0]?.body)).not.toContain("Be brief.");
    expect(JSON.stringify(seen[0]?.body)).not.toContain("sessionResumption");
    expect(await provider.reportedSeconds("anything")).toBeNull();
  });

  it("F-BE-08 an unknown voice falls back to the default, and the model and base can be set", async () => {
    const seen: Seen[] = [];
    const provider = new GeminiLive({
      apiKey: "k",
      baseUrl: "https://gemini.example/",
      model: "gemini-live-x",
      fetch: fakeFetch(authToken, seen),
      now: () => NOW,
    });
    const out = await provider.mint({ instructions: "x", voice: "marin", maxSeconds: 600 });
    expect(seen[0]?.url).toBe("https://gemini.example/v1alpha/auth_tokens");
    expect(out.wsUrl).toBe(
      "wss://gemini.example/ws/google.ai.generativelanguage.v1alpha.GenerativeService.BidiGenerateContentConstrained#model=models/gemini-live-x",
    );
    const setup = seen[0]?.body.bidiGenerateContentSetup as Record<string, unknown>;
    expect(setup.model).toBe("models/gemini-live-x");
    expect(setup.generationConfig).toMatchObject({
      speechConfig: { voiceConfig: { prebuiltVoiceConfig: { voiceName: "Puck" } } },
    });
    expect(seen[0]?.body.expireTime).toBe("2026-10-19T10:15:00.000Z");
  });

  it("F-BE-08 an error answer carries its status and never the body; no name is an error too", async () => {
    const seen: Seen[] = [];
    const quota = new GeminiLive({
      apiKey: "k",
      fetch: fakeFetch(() => new Response("RESOURCE_EXHAUSTED secret-body", { status: 429 }), seen),
    });
    const err = await quota.mint({ instructions: "x", maxSeconds: 1800 }).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(RealtimeUnavailableError);
    expect((err as RealtimeUnavailableError).status).toBe(429);
    expect(String((err as Error).message)).not.toContain("secret-body");
    const nameless = new GeminiLive({
      apiKey: "k",
      fetch: fakeFetch(() => new Response(JSON.stringify({}), { status: 200 }), seen),
    });
    const missing = await nameless.mint({ instructions: "x", maxSeconds: 1800 }).catch((e: unknown) => e);
    expect(missing).toBeInstanceOf(RealtimeUnavailableError);
    expect((missing as RealtimeUnavailableError).status).toBe(502);
  });

  it("F-BE-08 an unreachable vendor is unavailable, and the key never appears in the error", async () => {
    const provider = new GeminiLive({
      apiKey: "gk-secret",
      fetch: (async () => {
        throw new TypeError("fetch failed gk-secret");
      }) as typeof fetch,
    });
    const err = await provider.mint({ instructions: "x", maxSeconds: 1800 }).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(RealtimeUnavailableError);
    expect(String((err as Error).message)).not.toContain("gk-secret");
  });
});

describe("F-BE-08 choosing the providers (D-1013)", () => {
  it("F-BE-08 REALTIME_PROVIDERS orders them, each needs its key, and REALTIME_PROVIDER still picks one alone", () => {
    const chain = selectRealtime(
      loadEnv({ REALTIME_PROVIDERS: "gemini,openai", GEMINI_API_KEY: "g", OPENAI_API_KEY: "o" }),
    );
    expect(chain.map((p) => p.name)).toEqual(["gemini", "openai"]);
    expect(chain[0]).toBeInstanceOf(GeminiLive);
    expect(chain[1]).toBeInstanceOf(OpenAiRealtime);
    // A name twice counts once.
    expect(
      selectRealtime(loadEnv({ REALTIME_PROVIDERS: "gemini,gemini,mock", GEMINI_API_KEY: "g" })).map(
        (p) => p.name,
      ),
    ).toEqual(["gemini", "mock"]);
    expect(() =>
      selectRealtime(loadEnv({ REALTIME_PROVIDERS: "gemini,openai", OPENAI_API_KEY: "o" })),
    ).toThrow("GEMINI_API_KEY");
    expect(() => loadEnv({ REALTIME_PROVIDERS: "gemini,nope" })).toThrow();
    const alone = selectRealtime(loadEnv({}));
    expect(alone).toHaveLength(1);
    expect(alone[0]).toBeInstanceOf(MockRealtime);
  });
});
