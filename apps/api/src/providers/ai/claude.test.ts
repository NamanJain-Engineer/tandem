import { describe, expect, it } from "vitest";
import { loadEnv } from "../../env";
import { ANTHROPIC_VERSION, ClaudeAi } from "./claude";
import { selectAi } from "./index";
import { MockAi } from "./mock";
import { AiUnavailableError, type AiRequest } from "./types";

const REQ: AiRequest = {
  feature: "title",
  model: "claude-haiku-4-5-20251001",
  system: "You title things.",
  messages: [{ role: "user", content: "<text>\nfix the login tests\n</text>" }],
  maxTokens: 40,
};

type Seen = { url: string; init: RequestInit };

function fakeFetch(responses: Array<() => Response | Promise<Response>>, seen: Seen[] = []): typeof fetch {
  let i = 0;
  return (async (url: string | URL | Request, init?: RequestInit) => {
    seen.push({ url: String(url), init: init ?? {} });
    const next = responses[Math.min(i++, responses.length - 1)];
    if (!next) throw new Error("no response");
    return next();
  }) as typeof fetch;
}

const ok = (text: string) => () =>
  new Response(
    JSON.stringify({
      model: "claude-haiku-4-5-20251001",
      content: [{ type: "text", text }],
      usage: { input_tokens: 42, output_tokens: 7 },
    }),
    { status: 200, headers: { "content-type": "application/json" } },
  );
const status = (code: number) => () => new Response("secret-body-from-provider", { status: code });

describe("ClaudeAi (D-103)", () => {
  it("calls the Messages API with the key, version, model, system and messages", async () => {
    const seen: Seen[] = [];
    const ai = new ClaudeAi({ apiKey: "sk-test", fetch: fakeFetch([ok("Fix Login Tests")], seen) });
    const out = await ai.complete(REQ);
    expect(out).toEqual({
      text: "Fix Login Tests",
      tokensIn: 42,
      tokensOut: 7,
      model: "claude-haiku-4-5-20251001",
    });
    expect(seen).toHaveLength(1);
    expect(seen[0]?.url).toBe("https://api.anthropic.com/v1/messages");
    const headers = seen[0]?.init.headers as Record<string, string>;
    expect(headers["x-api-key"]).toBe("sk-test");
    expect(headers["anthropic-version"]).toBe(ANTHROPIC_VERSION);
    expect(JSON.parse(String(seen[0]?.init.body))).toEqual({
      model: REQ.model,
      max_tokens: 40,
      system: REQ.system,
      messages: REQ.messages,
    });
  });

  it("retries once when the API is overloaded or failing", async () => {
    const seen: Seen[] = [];
    const ai = new ClaudeAi({ apiKey: "k", fetch: fakeFetch([status(529), ok("Done")], seen) });
    expect((await ai.complete(REQ)).text).toBe("Done");
    expect(seen).toHaveLength(2);

    const twice: Seen[] = [];
    const failing = new ClaudeAi({ apiKey: "k", fetch: fakeFetch([status(500), status(503)], twice) });
    await expect(failing.complete(REQ)).rejects.toMatchObject({ status: 503 });
    expect(twice).toHaveLength(2);
  });

  it("doesn't retry a 4xx, and errors never carry the response body", async () => {
    const seen: Seen[] = [];
    const ai = new ClaudeAi({ apiKey: "k", fetch: fakeFetch([status(400)], seen) });
    const err = await ai.complete(REQ).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(AiUnavailableError);
    expect((err as AiUnavailableError).status).toBe(400);
    expect(String((err as Error).message)).not.toContain("secret-body");
    expect(seen).toHaveLength(1);
  });

  it("gives up after the timeout", async () => {
    const hang = (async (_url: string | URL | Request, init?: RequestInit) =>
      new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener("abort", () => reject(new Error("aborted")));
      })) as typeof fetch;
    const ai = new ClaudeAi({ apiKey: "k", fetch: hang, timeoutMs: 30, retries: 0 });
    await expect(ai.complete(REQ)).rejects.toThrow("timed out");
  });

  it("AI_PROVIDER picks the provider; claude without a key refuses to start", () => {
    expect(selectAi(loadEnv({}))).toBeInstanceOf(MockAi);
    expect(selectAi(loadEnv({ AI_PROVIDER: "claude", ANTHROPIC_API_KEY: "sk-x" }))).toBeInstanceOf(ClaudeAi);
    expect(() => selectAi(loadEnv({ AI_PROVIDER: "claude" }))).toThrow("ANTHROPIC_API_KEY");
  });
});

describe("ClaudeAi tool use and JSON output (F-CN-02, F-CN-05)", () => {
  const LIST_PANES = {
    name: "list_panes",
    description: "List the panes.",
    input_schema: {
      type: "object",
      properties: { workspace: { type: "string" } },
      additionalProperties: false,
    },
  };

  it("F-CN-02 sends the tools, tool choice, effort and a cached system, and returns the tool calls", async () => {
    const seen: Seen[] = [];
    const reply = () =>
      new Response(
        JSON.stringify({
          model: "claude-haiku-5-5",
          stop_reason: "tool_use",
          content: [
            { type: "thinking", thinking: "", signature: "sig" },
            { type: "text", text: "On it." },
            { type: "tool_use", id: "toolu_1", name: "list_panes", input: { workspace: "w1" } },
          ],
          usage: {
            input_tokens: 90,
            cache_read_input_tokens: 800,
            cache_creation_input_tokens: 10,
            output_tokens: 30,
          },
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    const ai = new ClaudeAi({ apiKey: "k", fetch: fakeFetch([reply], seen) });
    const req: AiRequest = {
      feature: "voice_act",
      model: "claude-haiku-5-5",
      system: "Doer rules.",
      messages: [{ role: "user", content: "<request>\nlist the panes\n</request>" }],
      maxTokens: 4096,
      tools: [LIST_PANES],
      toolChoice: "auto",
      effort: "low",
      cacheSystem: true,
    };
    expect(await ai.complete(req)).toEqual({
      text: "On it.",
      tokensIn: 900,
      tokensOut: 30,
      model: "claude-haiku-5-5",
      stopReason: "tool_use",
      toolCalls: [{ id: "toolu_1", name: "list_panes", input: { workspace: "w1" } }],
    });
    expect(JSON.parse(String(seen[0]?.init.body))).toEqual({
      model: "claude-haiku-5-5",
      max_tokens: 4096,
      system: [{ type: "text", text: "Doer rules.", cache_control: { type: "ephemeral" } }],
      messages: req.messages,
      tools: [LIST_PANES],
      tool_choice: { type: "auto" },
      output_config: { effort: "low" },
    });
  });

  it("F-CN-05 asks for JSON by schema, and leaves effort out for a model that doesn't take it", async () => {
    const seen: Seen[] = [];
    const ai = new ClaudeAi({ apiKey: "k", fetch: fakeFetch([ok('{"matches":[]}')], seen) });
    const schema = { type: "object", properties: {}, additionalProperties: false };
    const out = await ai.complete({ ...REQ, feature: "voice_resolve", effort: "low", jsonSchema: schema });
    expect(out.text).toBe('{"matches":[]}');
    expect(out.toolCalls).toBeUndefined();
    const body = JSON.parse(String(seen[0]?.init.body)) as Record<string, unknown>;
    expect(body.output_config).toEqual({ format: { type: "json_schema", schema } });
    expect(body.system).toBe(REQ.system);
    expect(body.tools).toBeUndefined();
  });
});
