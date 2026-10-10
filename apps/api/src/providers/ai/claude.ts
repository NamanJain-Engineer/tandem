import { type AiProvider, type AiRequest, type AiResult, type AiToolCall, AiUnavailableError } from "./types";

export const ANTHROPIC_VERSION = "2023-06-01";

/**
 * Models that take `output_config.effort`. Claude Haiku 4.5 and Sonnet 4.5
 * answer 400 to it, so a feature configured to one of them runs without.
 */
const TAKES_EFFORT = /^claude-(?:opus-4-[5-9]|sonnet-4-[6-9]|(?:opus|sonnet|haiku)-[5-9]|fable|mythos)/;

export function takesEffort(model: string): boolean {
  return TAKES_EFFORT.test(model);
}

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
  content?: Array<{ type: string; text?: string; id?: string; name?: string; input?: unknown }>;
  usage?: {
    input_tokens?: number;
    output_tokens?: number;
    cache_read_input_tokens?: number;
    cache_creation_input_tokens?: number;
  };
  model?: string;
  stop_reason?: string;
};

/** The request body; optional parts only when the feature asks for them. */
function bodyOf(req: AiRequest): string {
  const body: Record<string, unknown> = {
    model: req.model,
    max_tokens: req.maxTokens,
    system: req.cacheSystem
      ? [{ type: "text", text: req.system, cache_control: { type: "ephemeral" } }]
      : req.system,
    messages: req.messages,
  };
  if (req.tools?.length) {
    body.tools = req.tools;
    body.tool_choice = { type: req.toolChoice ?? "auto" };
  }
  const output: Record<string, unknown> = {};
  if (req.effort && takesEffort(req.model)) output.effort = req.effort;
  if (req.jsonSchema) output.format = { type: "json_schema", schema: req.jsonSchema };
  if (Object.keys(output).length > 0) body.output_config = output;
  return JSON.stringify(body);
}

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
    const body = bodyOf(req);
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
        const blocks = data.content ?? [];
        const text = blocks
          .filter((b) => b.type === "text")
          .map((b) => b.text ?? "")
          .join("")
          .trim();
        const toolCalls: AiToolCall[] = blocks
          .filter((b) => b.type === "tool_use" && typeof b.name === "string")
          .map((b) => ({ id: b.id ?? "", name: b.name ?? "", input: b.input }));
        const usage = data.usage ?? {};
        return {
          text,
          tokensIn:
            (usage.input_tokens ?? 0) +
            (usage.cache_read_input_tokens ?? 0) +
            (usage.cache_creation_input_tokens ?? 0),
          tokensOut: usage.output_tokens ?? 0,
          model: data.model ?? req.model,
          ...(toolCalls.length > 0 ? { toolCalls } : {}),
          ...(data.stop_reason ? { stopReason: data.stop_reason } : {}),
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
