import type { Env } from "../../env";
import { MockRealtime } from "./mock";
import { OpenAiRealtime } from "./openai";
import type { RealtimeProvider } from "./types";

export { MockRealtime } from "./mock";
export { OpenAiRealtime, OPENAI_REALTIME_VOICES } from "./openai";
export * from "./types";

/** `REALTIME_PROVIDER=openai` needs OPENAI_API_KEY; anything else is MockRealtime (ARCHITECTURE §17.5). */
export function selectRealtime(env: Env): RealtimeProvider {
  if (env.REALTIME_PROVIDER === "openai") {
    if (!env.OPENAI_API_KEY) throw new Error("REALTIME_PROVIDER=openai needs OPENAI_API_KEY");
    return new OpenAiRealtime({
      apiKey: env.OPENAI_API_KEY,
      baseUrl: env.OPENAI_API_BASE,
      model: env.REALTIME_MODEL,
    });
  }
  return new MockRealtime(env.API_PUBLIC_URL, env.REALTIME_MOCK_URL);
}
