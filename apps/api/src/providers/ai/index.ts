import type { Env } from "../../env";
import { ClaudeAi } from "./claude";
import { MockAi } from "./mock";
import type { AiProvider } from "./types";

export { ClaudeAi } from "./claude";
export { MockAi } from "./mock";
export * from "./types";

/** `AI_PROVIDER=claude` needs `ANTHROPIC_API_KEY`; anything else is `MockAi` (D-103). */
export function selectAi(env: Env): AiProvider {
  if (env.AI_PROVIDER === "claude") {
    if (!env.ANTHROPIC_API_KEY) throw new Error("AI_PROVIDER=claude needs ANTHROPIC_API_KEY");
    return new ClaudeAi({ apiKey: env.ANTHROPIC_API_KEY, baseUrl: env.ANTHROPIC_API_BASE });
  }
  return new MockAi();
}
