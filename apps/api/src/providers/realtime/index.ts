import type { Env } from "../../env";
import { GeminiLive } from "./gemini";
import { MockRealtime } from "./mock";
import { OpenAiRealtime } from "./openai";
import type { RealtimeProvider, RealtimeProviderName } from "./types";

export { GEMINI_LIVE_VOICES, GeminiLive } from "./gemini";
export { MockRealtime } from "./mock";
export { OpenAiRealtime, OPENAI_REALTIME_VOICES } from "./openai";
export * from "./types";

/**
 * The providers in order of preference (D-1013): `REALTIME_PROVIDERS`
 * (`gemini,openai`), else `REALTIME_PROVIDER` alone (ARCHITECTURE §17.5).
 * A vendor without its key refuses to start; anything else is MockRealtime.
 */
export function selectRealtime(env: Env): RealtimeProvider[] {
  const names: RealtimeProviderName[] = env.REALTIME_PROVIDERS
    ? [...new Set(env.REALTIME_PROVIDERS.split(",") as RealtimeProviderName[])]
    : [env.REALTIME_PROVIDER];
  return names.map((name) => provider(env, name));
}

function provider(env: Env, name: RealtimeProviderName): RealtimeProvider {
  switch (name) {
    case "openai":
      if (!env.OPENAI_API_KEY) throw new Error("REALTIME_PROVIDER=openai needs OPENAI_API_KEY");
      return new OpenAiRealtime({
        apiKey: env.OPENAI_API_KEY,
        baseUrl: env.OPENAI_API_BASE,
        model: env.REALTIME_MODEL,
      });
    case "gemini":
      if (!env.GEMINI_API_KEY) throw new Error("the gemini realtime provider needs GEMINI_API_KEY");
      return new GeminiLive({
        apiKey: env.GEMINI_API_KEY,
        baseUrl: env.GEMINI_API_BASE,
        model: env.GEMINI_LIVE_MODEL,
      });
    case "mock":
      return new MockRealtime(env.API_PUBLIC_URL, env.REALTIME_MOCK_URL);
  }
}
