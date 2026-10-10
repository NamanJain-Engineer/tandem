import { Readable } from "node:stream";
import { randomToken } from "../../lib/ids";
import { type SttOptions, type SttProvider, type SttResult, SttUnavailableError } from "./types";

type Options = {
  apiKey: string;
  /** The vendor's OpenAI-compatible base, e.g. `https://api.groq.com/openai`. */
  baseUrl: string;
  model: string;
  fetch?: typeof fetch;
  timeoutMs?: number;
};

/**
 * An OpenAI-compatible `/v1/audio/transcriptions` endpoint (OpenAI, Groq and
 * others). The multipart body is built around the incoming stream, so the
 * audio goes straight through. Errors carry the status only, never a body.
 */
export class OpenAiCompatStt implements SttProvider {
  readonly name: string;
  private readonly fetchFn: typeof fetch;

  constructor(private readonly opts: Options) {
    if (!opts.apiKey) throw new Error("STT provider needs STT_API_KEY");
    this.name = new URL(opts.baseUrl).hostname;
    this.fetchFn = opts.fetch ?? fetch;
  }

  async transcribe(audio: Readable, o: SttOptions): Promise<SttResult> {
    const boundary = `tandem-${randomToken(12)}`;
    const field = (name: string, value: string) =>
      `--${boundary}\r\nContent-Disposition: form-data; name="${name}"\r\n\r\n${value}\r\n`;
    let head = field("model", this.opts.model) + field("response_format", "json");
    if (o.language) head += field("language", o.language);
    if (o.hints?.length) head += field("prompt", o.hints.join(", "));
    const file = o.filename.replace(/["\r\n]/g, "") || "audio";
    head += `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="${file}"\r\nContent-Type: ${o.mime}\r\n\r\n`;
    const tail = `\r\n--${boundary}--\r\n`;
    async function* body() {
      yield Buffer.from(head);
      for await (const chunk of audio) yield chunk as Buffer;
      yield Buffer.from(tail);
    }
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.opts.timeoutMs ?? 120_000);
    let res: Response;
    try {
      res = await this.fetchFn(`${this.opts.baseUrl.replace(/\/$/, "")}/v1/audio/transcriptions`, {
        method: "POST",
        headers: {
          authorization: `Bearer ${this.opts.apiKey}`,
          "content-type": `multipart/form-data; boundary=${boundary}`,
        },
        body: Readable.toWeb(Readable.from(body())) as unknown as BodyInit,
        signal: controller.signal,
        duplex: "half",
      } as RequestInit);
    } catch {
      throw new SttUnavailableError(
        controller.signal.aborted ? "the STT provider timed out" : "couldn't reach the STT provider",
      );
    } finally {
      clearTimeout(timer);
    }
    if (!res.ok) {
      await res.body?.cancel().catch(() => undefined);
      throw new SttUnavailableError(`the STT provider answered ${res.status}`, res.status);
    }
    const data = (await res.json()) as { text?: string; language?: string };
    return { text: (data.text ?? "").trim(), language: data.language ?? o.language ?? null };
  }
}
