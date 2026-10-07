import { type AiProvider, type AiRequest, type AiResult, AiUnavailableError } from "./types";

export const ANTHROPIC_VERSION = "2023-06-01";

type Options = {
  apiKey: string;
  /** `https://api.anthropic.com` unless a test points it elsewhere. */
  baseUrl?: string;
  fetch?: typeof fetch;
  timeoutMs?: number;
  /** Retries after a 5xx, 529 (overloaded) or network error. */
  retries?: number;
};

type MessagesResponse = {
  content?: Array<{ type: string; text?: string }>;
  usage?: { input_tokens?: number; output_tokens?: number };
  model?: string;
};

/**
 * The Claude Messages API, first-party so startup-program credits apply
 * (D-103). 30 s per attempt and one retry on 5xx or overloaded; 4xx answers
 * fail at once. Errors carry the HTTP status only, never a body.
 */
export class ClaudeAi implements AiProvider {
  readonly name = "claude" as const;
  private readonly baseUrl: string;
  private readonly fetchFn: typeof fetch;
  private readonly timeoutMs: number;
  private readonly retries: number;

  constructor(private readonly opts: Options) {
    if (!opts.apiKey) throw new Error("ClaudeAi needs ANTHROPIC_API_KEY");
    this.baseUrl = (opts.baseUrl ?? "https://api.anthropic.com").replace(/\/$/, "");
    this.fetchFn = opts.fetch ?? fetch;
    this.timeoutMs = opts.timeoutMs ?? 30_000;
    this.retries = opts.retries ?? 1;
  }

  async complete(req: AiRequest): Promise<AiResult> {
    const body = JSON.stringify({
      model: req.model,
      max_tokens: req.maxTokens,
      system: req.system,
      messages: req.messages,
    });
    let last: AiUnavailableError | null = null;
    for (let attempt = 0; attempt <= this.retries; attempt++) {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), this.timeoutMs);
      let res: Response;
      try {
        res = await this.fetchFn(`${this.baseUrl}/v1/messages`, {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "x-api-key": this.opts.apiKey,
            "anthropic-version": ANTHROPIC_VERSION,
          },
          body,
          signal: controller.signal,
        });
      } catch {
        last = new AiUnavailableError(
          controller.signal.aborted ? "the Claude API timed out" : "couldn't reach the Claude API",
        );
        continue;
      } finally {
        clearTimeout(timer);
      }
      if (res.ok) {
        const data = (await res.json()) as MessagesResponse;
        const text = (data.content ?? [])
          .filter((b) => b.type === "text")
          .map((b) => b.text ?? "")
          .join("")
          .trim();
        return {
          text,
          tokensIn: data.usage?.input_tokens ?? 0,
          tokensOut: data.usage?.output_tokens ?? 0,
          model: data.model ?? req.model,
        };
      }
      // Drain without reading the body into any log or error.
      await res.body?.cancel().catch(() => undefined);
      last = new AiUnavailableError(`the Claude API answered ${res.status}`, res.status);
      const retryable = res.status >= 500 || res.status === 529;
      if (!retryable) break;
    }
    throw last ?? new AiUnavailableError("the Claude API didn't answer");
  }
}
