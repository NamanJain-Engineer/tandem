import { Readable } from "node:stream";
import { describe, expect, it } from "vitest";
import { OpenAiCompatStt } from "./openai";
import { SttUnavailableError } from "./types";

describe("F-BE-07 OpenAI-compatible STT provider", () => {
  it("streams the audio in a multipart body with the model, language and hints", async () => {
    let seen: { url: string; headers: Headers; body: string } | null = null;
    const fetchFn = (async (url: string, init: RequestInit) => {
      const body = await new Response(init.body as BodyInit).text();
      seen = { url, headers: new Headers(init.headers), body };
      return new Response(JSON.stringify({ text: " hello there ", language: "en" }), { status: 200 });
    }) as unknown as typeof fetch;
    const stt = new OpenAiCompatStt({
      apiKey: "k",
      baseUrl: "https://stt.example/",
      model: "whisper-x",
      fetch: fetchFn,
    });
    const out = await stt.transcribe(Readable.from([Buffer.from("AUDIO1"), Buffer.from("AUDIO2")]), {
      mime: "audio/ogg",
      filename: "take.opus",
      language: "en",
      hints: ["Vibezeo"],
    });
    expect(out).toEqual({ text: "hello there", language: "en" });
    const s = seen as unknown as { url: string; headers: Headers; body: string };
    expect(s.url).toBe("https://stt.example/v1/audio/transcriptions");
    expect(s.headers.get("authorization")).toBe("Bearer k");
    expect(s.body).toContain('name="model"\r\n\r\nwhisper-x');
    expect(s.body).toContain('name="prompt"\r\n\r\nVibezeo');
    expect(s.body).toContain("AUDIO1AUDIO2");
    expect(stt.name).toBe("stt.example");
  });

  it("an error answer carries its status and no body", async () => {
    const fetchFn = (async () =>
      new Response("secret transcript", { status: 500 })) as unknown as typeof fetch;
    const stt = new OpenAiCompatStt({
      apiKey: "k",
      baseUrl: "https://stt.example",
      model: "m",
      fetch: fetchFn,
    });
    const err = await stt
      .transcribe(Readable.from([Buffer.from("x")]), { mime: "audio/wav", filename: "a.wav" })
      .catch((e: unknown) => e);
    expect(err).toBeInstanceOf(SttUnavailableError);
    expect((err as SttUnavailableError).status).toBe(500);
    expect(String((err as Error).message)).not.toContain("secret");
  });
});
