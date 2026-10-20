import {
  ACT_TOOL,
  type MintRequest,
  type Minted,
  type RealtimeProvider,
  RealtimeUnavailableError,
} from "./types";

/** The Live API's prebuilt voices; another name falls back to the first. */
export const GEMINI_LIVE_VOICES = [
  "Puck",
  "Charon",
  "Kore",
  "Fenrir",
  "Aoede",
  "Leda",
  "Orus",
  "Zephyr",
] as const;

type Options = {
  apiKey: string;
  /** `https://generativelanguage.googleapis.com` unless a test or a proxy points elsewhere. */
  baseUrl?: string;
  model?: string;
  fetch?: typeof fetch;
  timeoutMs?: number;
  /** Tests: the clock the token's times come from. */
  now?: () => number;
};

/** The first connection must open within this. */
const NEW_SESSION_MS = 60_000;
/** Messages are accepted until the call's cap plus this. */
const EXPIRE_GRACE_MS = 5 * 60_000;

/** What the token locks; the app sends its own instructions and session resumption handle. */
const LOCKED = [
  "model",
  "generationConfig.responseModalities",
  "generationConfig.speechConfig",
  "tools",
  "inputAudioTranscription",
  "outputAudioTranscription",
  "contextWindowCompression",
].join(",");

/**
 * Google's Gemini Live API through an ephemeral token (D-1014). The API mints
 * a token that locks the model, spoken answers in the call's voice, the `act`
 * tool, both transcripts and context compression, and the app opens the
 * constrained socket with it, sending its instructions in the setup message.
 * One use, the first connection within a minute, messages until the call's
 * cap plus 5 minutes: resuming a session isn't a use, so the app reconnects
 * every ~10 minutes on the same token. The model rides in the URL's fragment,
 * which the app strips before connecting. Errors carry the status only.
 */
export class GeminiLive implements RealtimeProvider {
  readonly name = "gemini" as const;
  private readonly baseUrl: string;
  private readonly model: string;
  private readonly fetchFn: typeof fetch;
  private readonly now: () => number;

  constructor(private readonly opts: Options) {
    if (!opts.apiKey) throw new Error("GeminiLive needs GEMINI_API_KEY");
    this.baseUrl = (opts.baseUrl ?? "https://generativelanguage.googleapis.com").replace(/\/$/, "");
    this.model = opts.model ?? "gemini-3.1-flash-live-preview";
    this.fetchFn = opts.fetch ?? fetch;
    this.now = opts.now ?? Date.now;
  }

  async mint(req: MintRequest): Promise<Minted> {
    const wanted = req.voice?.toLowerCase();
    const voice = GEMINI_LIVE_VOICES.find((v) => v.toLowerCase() === wanted) ?? GEMINI_LIVE_VOICES[0];
    const model = `models/${this.model}`;
    const now = this.now();
    const body = JSON.stringify({
      uses: 1,
      newSessionExpireTime: new Date(now + NEW_SESSION_MS).toISOString(),
      expireTime: new Date(now + req.maxSeconds * 1000 + EXPIRE_GRACE_MS).toISOString(),
      bidiGenerateContentSetup: {
        model,
        generationConfig: {
          responseModalities: ["AUDIO"],
          speechConfig: { voiceConfig: { prebuiltVoiceConfig: { voiceName: voice } } },
        },
        // The Live API's schema has no `additionalProperties`.
        tools: [
          {
            functionDeclarations: [
              {
                name: ACT_TOOL.name,
                description: ACT_TOOL.description,
                parameters: {
                  type: ACT_TOOL.parameters.type,
                  properties: ACT_TOOL.parameters.properties,
                  required: ACT_TOOL.parameters.required,
                },
              },
            ],
          },
        ],
        inputAudioTranscription: {},
        outputAudioTranscription: {},
        contextWindowCompression: { slidingWindow: {} },
      },
      fieldMask: LOCKED,
    });
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.opts.timeoutMs ?? 15_000);
    let res: Response;
    try {
      res = await this.fetchFn(`${this.baseUrl}/v1alpha/auth_tokens`, {
        method: "POST",
        headers: { "x-goog-api-key": this.opts.apiKey, "content-type": "application/json" },
        body,
        signal: controller.signal,
      });
    } catch {
      throw new RealtimeUnavailableError(
        controller.signal.aborted
          ? "the realtime provider timed out"
          : "couldn't reach the realtime provider",
      );
    } finally {
      clearTimeout(timer);
    }
    if (!res.ok) {
      await res.body?.cancel().catch(() => undefined);
      throw new RealtimeUnavailableError(`the realtime provider answered ${res.status}`, res.status);
    }
    const data = (await res.json()) as { name?: string };
    if (!data.name) throw new RealtimeUnavailableError("the realtime provider sent no token", 502);
    return {
      token: data.name,
      wsUrl: `${this.baseUrl.replace(/^http/, "ws")}/ws/google.ai.generativelanguage.v1alpha.GenerativeService.BidiGenerateContentConstrained#model=${model}`,
    };
  }

  /** The Live API has no per-session usage report, so the sweeper charges the hold. */
  async reportedSeconds(_providerSessionId: string): Promise<number | null> {
    return null;
  }
}
