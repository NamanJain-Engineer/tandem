import { randomToken } from "../../lib/ids";
import { type MintRequest, type Minted, type RealtimeProvider, RealtimeUnavailableError } from "./types";

/**
 * Dev and tests (ARCHITECTURE §14.8): a random token and a URL on the API's
 * dev realtime mock, `ws://<api>/v1/dev/realtime?token=…`.
 */
export class MockRealtime implements RealtimeProvider {
  readonly name = "mock" as const;
  calls: MintRequest[] = [];
  /** Tests: make the next mint fail. */
  failNext = false;
  /** Tests: what the "provider" reports per session id; unset means no report. */
  readonly usage = new Map<string, number>();

  /** `mockUrl`: the dev mock socket (`scripts/mock-realtime.ts`) instead of the API's own path. */
  constructor(
    private readonly apiPublicUrl: string,
    private readonly mockUrl?: string,
  ) {}

  async mint(req: MintRequest): Promise<Minted> {
    this.calls.push(req);
    if (this.failNext) {
      this.failNext = false;
      throw new RealtimeUnavailableError("mock failure", 503);
    }
    const token = `rt_mock_${randomToken(24)}`;
    const base = this.apiPublicUrl.replace(/\/$/, "").replace(/^http/, "ws");
    const url = this.mockUrl ?? `${base}/v1/dev/realtime`;
    return {
      token,
      wsUrl: `${url}?token=${encodeURIComponent(token)}`,
      providerSessionId: `mock_sess_${randomToken(9)}`,
    };
  }

  async reportedSeconds(providerSessionId: string): Promise<number | null> {
    return this.usage.get(providerSessionId) ?? null;
  }
}
