import type { Env } from "../../env";
import { MockStt } from "./mock";
import { OpenAiCompatStt } from "./openai";
import type { SttProvider } from "./types";

export { MockStt } from "./mock";
export { OpenAiCompatStt } from "./openai";
export * from "./types";

/** `STT_PROVIDER=openai_compat` needs STT_API_KEY, STT_API_BASE and STT_MODEL; anything else is MockStt. */
export function selectStt(env: Env): SttProvider {
  if (env.STT_PROVIDER === "openai_compat") {
    if (!env.STT_API_KEY || !env.STT_API_BASE || !env.STT_MODEL) {
      throw new Error("STT_PROVIDER=openai_compat needs STT_API_KEY, STT_API_BASE and STT_MODEL");
    }
    return new OpenAiCompatStt({ apiKey: env.STT_API_KEY, baseUrl: env.STT_API_BASE, model: env.STT_MODEL });
  }
  return new MockStt();
}
