// Mints one real Gemini Live token from GEMINI_API_KEY with the API's own
// GeminiLive, as POST /v1/voice/session would, for
// scripts/agent/gemini-live-check.sh (HUMAN_TODO "Check against a real
// session"). Writes {ws_url, token} to the file it's given, readable only by
// its owner, and never prints the token. When Google refuses, prints Google's
// error, which names the field it didn't accept.
import { writeFileSync } from "node:fs";
import { GeminiLive } from "../src/providers/realtime/gemini";
import { MAX_CALL_SECONDS, TALKER_INSTRUCTIONS } from "../src/services/voice";

const out = process.argv[2];
const apiKey = process.env.GEMINI_API_KEY;
if (!out || !apiKey) {
  console.error("usage: GEMINI_API_KEY=… tsx scripts/gemini-live-token.ts <session.json>");
  process.exit(2);
}

let refusal = "";
const keepRefusal: typeof fetch = async (url, init) => {
  const res = await fetch(url, init);
  if (!res.ok) refusal = (await res.clone().text()).slice(0, 2000);
  return res;
};

const gemini = new GeminiLive({
  apiKey,
  baseUrl: process.env.GEMINI_API_BASE,
  model: process.env.GEMINI_LIVE_MODEL,
  fetch: keepRefusal,
});
try {
  const minted = await gemini.mint({
    instructions: TALKER_INSTRUCTIONS,
    voice: process.env.GEMINI_LIVE_VOICE,
    maxSeconds: MAX_CALL_SECONDS,
  });
  writeFileSync(out, JSON.stringify({ ws_url: minted.wsUrl, token: minted.token }), { mode: 0o600 });
  console.log(`minted a Gemini Live token (${minted.wsUrl.split("#")[1] ?? "no model"})`);
} catch (err) {
  console.error(`couldn't mint: ${err instanceof Error ? err.message : String(err)}`);
  if (refusal) console.error(`Google said: ${refusal}`);
  process.exit(1);
}
