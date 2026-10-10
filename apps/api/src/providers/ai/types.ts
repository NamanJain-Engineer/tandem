/** The hosted AI features that call a model (F-BE-06). */
export type AiFeature = "title" | "rewrite" | "teammate_draft" | "auto_next";

export type AiMessage = { role: "user" | "assistant"; content: string };

export type AiRequest = {
  feature: AiFeature;
  model: string;
  system: string;
  messages: AiMessage[];
  maxTokens: number;
};

export type AiResult = { text: string; tokensIn: number; tokensOut: number; model: string };

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
