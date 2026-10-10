/** The mock's fixed teammate draft (ARCHITECTURE §14.8). */
export const MOCK_TEAMMATE_DRAFT = {
  name: "Release Notes Writer",
  brief:
    "You draft the weekly changelog from merged pull requests. Verify every claim against the repository. Never publish without approval.",
  engine: "claude",
  plugins: ["github"],
} as const;

import { type AiProvider, type AiRequest, type AiResult, AiUnavailableError, taggedText } from "./types";

/**
 * Deterministic answers for dev and tests (ARCHITECTURE §14.8): a title is
 * the first 6 words title-cased; a rewrite is the input with whitespace
 * normalized; a teammate draft is `MOCK_TEAMMATE_DRAFT`.
 */
export class MockAi implements AiProvider {
  readonly name = "mock" as const;
  calls: AiRequest[] = [];
  /** Tests: make the next call fail. */
  failNext = false;

  async complete(req: AiRequest): Promise<AiResult> {
    this.calls.push(req);
    if (this.failNext) {
      this.failNext = false;
      throw new AiUnavailableError("mock failure", 503);
    }
    const last = req.messages.at(-1)?.content ?? "";
    const input = taggedText(last) ?? last;
    const text =
      req.feature === "teammate_draft"
        ? JSON.stringify(MOCK_TEAMMATE_DRAFT)
        : req.feature === "title"
          ? input
              .split(/\s+/)
              .filter(Boolean)
              .slice(0, 6)
              .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
              .join(" ")
          : input.replace(/\s+/g, " ").trim();
    return {
      text,
      tokensIn: Math.ceil(last.length / 4),
      tokensOut: Math.ceil(text.length / 4),
      model: req.model,
    };
  }
}
