import { type AuthorizeInput, type ExchangeInput, type Fetch, SsoError, type SsoProfile, type SsoProvider } from "./types";

export const WORKOS_API = "https://api.workos.com";

/**
 * SAML through WorkOS (D-625): WorkOS speaks SAML to the customer's identity
 * provider and hands back a profile for an authorization code. The server's
 * WorkOS client id and API key come from the environment (HUMAN_TODO).
 */
export class WorkOsSso implements SsoProvider {
  constructor(
    private readonly conn: { organization: string },
    private readonly keys: { clientId: string; apiKey: string },
    private readonly fetchImpl: Fetch,
    private readonly base = WORKOS_API,
  ) {}

  async check(): Promise<void> {
    if (!/^org_[A-Za-z0-9]+$/.test(this.conn.organization)) {
      throw new SsoError("SSO_CONFIG", "A WorkOS organization id looks like org_01H….");
    }
  }

  async authorizeUrl(input: AuthorizeInput): Promise<string> {
    const url = new URL("/sso/authorize", this.base);
    url.searchParams.set("client_id", this.keys.clientId);
    url.searchParams.set("redirect_uri", input.redirectUri);
    url.searchParams.set("response_type", "code");
    url.searchParams.set("state", input.state);
    url.searchParams.set("organization", this.conn.organization);
    if (input.loginHint) url.searchParams.set("login_hint", input.loginHint);
    return url.toString();
  }

  async exchange(input: ExchangeInput): Promise<SsoProfile> {
    let body: { profile?: Record<string, unknown> };
    try {
      const res = await this.fetchImpl(new URL("/sso/token", this.base).toString(), {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({
          client_id: this.keys.clientId,
          client_secret: this.keys.apiKey,
          grant_type: "authorization_code",
          code: input.code,
        }).toString(),
        signal: AbortSignal.timeout(10_000),
      });
      if (!res.ok) throw new Error(String(res.status));
      body = (await res.json()) as typeof body;
    } catch {
      throw new SsoError("SSO_EXCHANGE", "WorkOS didn't accept the sign-in.");
    }
    const p = body.profile ?? {};
    if (p.organization_id !== this.conn.organization) {
      throw new SsoError("SSO_TOKEN", "That sign-in is for another organization.");
    }
    if (typeof p.id !== "string" || typeof p.email !== "string" || !p.email) {
      throw new SsoError("SSO_TOKEN", "WorkOS sent no email address.");
    }
    const name = [p.first_name, p.last_name].filter((x): x is string => typeof x === "string" && x !== "").join(" ");
    return { subject: p.id, email: p.email.trim().toLowerCase(), name: name || null };
  }
}
