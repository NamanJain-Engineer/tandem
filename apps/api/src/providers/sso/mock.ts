import type { Redis } from "ioredis";
import { randomToken } from "../../lib/ids";
import { type AuthorizeInput, type ExchangeInput, SsoError, type SsoProfile, type SsoProvider } from "./types";

const NEXT = "idp:next";
const code = (c: string) => `idp:code:${c}`;

/**
 * The test identity provider (`/__test/idp/*`): it signs in whoever the test
 * set with `/__test/idp/next` and hands the code back at once.
 */
export class MockSso implements SsoProvider {
  constructor(
    private readonly redis: Redis,
    private readonly apiUrl: string,
  ) {}

  async check(): Promise<void> {}

  async authorizeUrl(input: AuthorizeInput): Promise<string> {
    const url = new URL("/__test/idp/authorize", this.apiUrl);
    url.searchParams.set("state", input.state);
    url.searchParams.set("redirect_uri", input.redirectUri);
    return url.toString();
  }

  async exchange(input: ExchangeInput): Promise<SsoProfile> {
    const raw = await this.redis.getdel(code(input.code));
    if (!raw) throw new SsoError("SSO_EXCHANGE", "The test identity provider doesn't know that code.");
    return JSON.parse(raw) as SsoProfile;
  }
}

/** Test routes: who signs in next, and the code for it. */
export async function mockIdpNext(redis: Redis, profile: SsoProfile): Promise<void> {
  await redis.set(NEXT, JSON.stringify(profile), "PX", 600_000);
}

export async function mockIdpCode(redis: Redis): Promise<string | null> {
  const next = await redis.get(NEXT);
  if (!next) return null;
  const c = randomToken();
  await redis.set(code(c), next, "PX", 600_000);
  return c;
}
