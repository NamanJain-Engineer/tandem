import {
  ACT_TOOL,
  type MintRequest,
  type Minted,
  type RealtimeProvider,
  RealtimeUnavailableError,
} from "./types";

/** The vendor's voices; another name falls back to the first. */
export const OPENAI_REALTIME_VOICES = [
  "marin",
  "cedar",
  "alloy",
  "ash",
  "ballad",
  "coral",
  "echo",
  "sage",
  "shimmer",
  "verse",
] as const;

type Options = {
  apiKey: string;
  /** `https://api.openai.com` unless a test or a proxy points elsewhere. */
  baseUrl?: string;
  model?: string;
  fetch?: typeof fetch;
  timeoutMs?: number;
};

type ClientSecret = { value?: string; session?: { id?: string } };

/**
 * OpenAI's Realtime API through its client-secret endpoint: the API mints a
 * short-lived key for one session (instructions, voice and the `act` tool set
 * here) and the app connects to the socket with it. The token is valid for
 * opening the socket for 10 minutes. Errors carry the status only, never a body.
 */
export class OpenAiRealtime implements RealtimeProvider {
  readonly name = "openai" as const;
  private readonly baseUrl: string;
  private readonly model: string;
  private readonly fetchFn: typeof fetch;

  constructor(private readonly opts: Options) {
    if (!opts.apiKey) throw new Error("OpenAiRealtime needs OPENAI_API_KEY");
    this.baseUrl = (opts.baseUrl ?? "https://api.openai.com").replace(/\/$/, "");
    this.model = opts.model ?? "gpt-realtime";
    this.fetchFn = opts.fetch ?? fetch;
  }

  async mint(req: MintRequest): Promise<Minted> {
    const voice = OPENAI_REALTIME_VOICES.find((v) => v === req.voice) ?? OPENAI_REALTIME_VOICES[0];
    const body = JSON.stringify({
      expires_after: { anchor: "created_at", seconds: 600 },
      session: {
        type: "realtime",
        model: this.model,
        instructions: req.instructions,
        audio: { output: { voice } },
        tools: [{ type: "function", ...ACT_TOOL }],
        tool_choice: "auto",
      },
    });
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.opts.timeoutMs ?? 15_000);
    let res: Response;
    try {
      res = await this.fetchFn(`${this.baseUrl}/v1/realtime/client_secrets`, {
        method: "POST",
        headers: { authorization: `Bearer ${this.opts.apiKey}`, "content-type": "application/json" },
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
    const data = (await res.json()) as ClientSecret;
    if (!data.value) throw new RealtimeUnavailableError("the realtime provider sent no client secret", 502);
    return {
      token: data.value,
      wsUrl: `${this.baseUrl.replace(/^http/, "ws")}/v1/realtime?model=${encodeURIComponent(this.model)}`,
      ...(data.session?.id ? { providerSessionId: data.session.id } : {}),
    };
  }

  /** The vendor has no per-session usage report, so the sweeper charges the hold. */
  async reportedSeconds(_providerSessionId: string): Promise<number | null> {
    return null;
  }
}
