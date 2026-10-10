/** The hosted AI features that call a model (F-BE-06). */
export type AiFeature = "title" | "rewrite" | "teammate_draft" | "auto_next" | "voice_act" | "voice_resolve";

export type AiMessage = { role: "user" | "assistant"; content: string };

/** A tool the model may call, as the Messages API takes it. */
export type AiTool = { name: string; description: string; input_schema: Record<string, unknown> };

/** A tool call the model made. The caller checks `input` before using it. */
export type AiToolCall = { id: string; name: string; input: unknown };

export type AiRequest = {
  feature: AiFeature;
  model: string;
  system: string;
  messages: AiMessage[];
  maxTokens: number;
  /** Tools the model may call (the Conductor doer, F-CN-02). */
  tools?: AiTool[];
  /** `none` makes the model answer in text; the default with tools is `auto`. */
  toolChoice?: "auto" | "none";
  /** How hard the model thinks, on models that take it (left out on the others). */
  effort?: "low" | "medium" | "high";
  /** The answer must be JSON that fits this schema (structured outputs). */
  jsonSchema?: Record<string, unknown>;
  /** Caches the tools and the system prompt, for features that send the same ones every call. */
  cacheSystem?: boolean;
};

export type AiResult = {
  text: string;
  tokensIn: number;
  tokensOut: number;
  model: string;
  /** Set when the model called tools. */
  toolCalls?: AiToolCall[];
  /** Why the model stopped (`end_turn`, `tool_use`, `max_tokens`, `refusal`…), when the provider says. */
  stopReason?: string;
};

/**
 * `ClaudeAi` in production (the Claude Messages API, D-103), `MockAi` when no
 * key is set. Implementations never log request or response bodies.
 */
export interface AiProvider {
  readonly name: "mock" | "claude";
  complete(req: AiRequest): Promise<AiResult>;
}

/** The provider couldn't answer: timed out, overloaded, refused or misconfigured. */
export class AiUnavailableError extends Error {
  constructor(
    message: string,
    public readonly status?: number,
  ) {
    super(message);
    this.name = "AiUnavailableError";
  }
}

/** The text a prompt passes as data, between `<text>` tags. */
export function taggedText(content: string): string | null {
  const m = content.match(/<text>\n?([\s\S]*?)\n?<\/text>/);
  return m?.[1] ?? null;
}

/** The text between `<tag>` and `</tag>` in a prompt, or null. */
export function tagged(content: string, tag: string): string | null {
  const m = content.match(new RegExp(`<${tag}>\\n?([\\s\\S]*?)\\n?</${tag}>`));
  return m?.[1] ?? null;
}
