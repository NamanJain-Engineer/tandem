import { createLocalJWKSet, jwtVerify, type JSONWebKeySet } from "jose";
import { type AuthorizeInput, type ExchangeInput, type Fetch, SsoError, type SsoProfile, type SsoProvider } from "./types";

type Discovery = {
  issuer: string;
  authorization_endpoint: string;
  token_endpoint: string;
  jwks_uri: string;
  userinfo_endpoint?: string;
  token_endpoint_auth_methods_supported?: string[];
};

const TIMEOUT_MS = 10_000;

/**
 * Any OpenID Connect provider (Okta, Microsoft Entra ID, Google Workspace…):
 * discovery, the authorization code flow with PKCE and a nonce, and an ID
 * token checked for its signature, issuer, audience, expiry and nonce.
 */
export class OidcSso implements SsoProvider {
  private discovery: Discovery | null = null;

  constructor(
    private readonly conn: { issuer: string; clientId: string; clientSecret: string },
    private readonly fetchImpl: Fetch,
  ) {}

  private async get(url: string, init?: RequestInit): Promise<Response> {
    return this.fetchImpl(url, { ...init, signal: AbortSignal.timeout(TIMEOUT_MS) });
  }

  private async doc(): Promise<Discovery> {
    if (this.discovery) return this.discovery;
    const issuer = this.conn.issuer.replace(/\/+$/, "");
    let doc: Partial<Discovery>;
    try {
      const res = await this.get(`${issuer}/.well-known/openid-configuration`);
      if (!res.ok) throw new Error(String(res.status));
      doc = (await res.json()) as Partial<Discovery>;
    } catch {
      throw new SsoError("SSO_CONFIG", "Couldn't read the identity provider's OpenID configuration.");
    }
    if (
      !doc.issuer ||
      doc.issuer.replace(/\/+$/, "") !== issuer ||
      !doc.authorization_endpoint ||
      !doc.token_endpoint ||
      !doc.jwks_uri
    ) {
      throw new SsoError("SSO_CONFIG", "The identity provider's OpenID configuration is incomplete or for another issuer.");
    }
    this.discovery = doc as Discovery;
    return this.discovery;
  }

  async check(): Promise<void> {
    await this.doc();
  }

  async authorizeUrl(input: AuthorizeInput): Promise<string> {
    const doc = await this.doc();
    const url = new URL(doc.authorization_endpoint);
    url.searchParams.set("client_id", this.conn.clientId);
    url.searchParams.set("response_type", "code");
    url.searchParams.set("scope", "openid email profile");
    url.searchParams.set("redirect_uri", input.redirectUri);
    url.searchParams.set("state", input.state);
    url.searchParams.set("nonce", input.nonce);
    url.searchParams.set("code_challenge", input.codeChallenge);
    url.searchParams.set("code_challenge_method", "S256");
    if (input.loginHint) url.searchParams.set("login_hint", input.loginHint);
    return url.toString();
  }

  async exchange(input: ExchangeInput): Promise<SsoProfile> {
    const doc = await this.doc();
    const form = new URLSearchParams({
      grant_type: "authorization_code",
      code: input.code,
      redirect_uri: input.redirectUri,
      code_verifier: input.codeVerifier,
    });
    const headers: Record<string, string> = { "content-type": "application/x-www-form-urlencoded", accept: "application/json" };
    const methods = doc.token_endpoint_auth_methods_supported ?? ["client_secret_basic"];
    if (methods.includes("client_secret_basic")) {
      const basic = `${encodeURIComponent(this.conn.clientId)}:${encodeURIComponent(this.conn.clientSecret)}`;
      headers.authorization = `Basic ${Buffer.from(basic).toString("base64")}`;
    } else {
      form.set("client_id", this.conn.clientId);
      form.set("client_secret", this.conn.clientSecret);
    }
    let tokens: { id_token?: string; access_token?: string };
    try {
      const res = await this.get(doc.token_endpoint, { method: "POST", headers, body: form.toString() });
      if (!res.ok) throw new Error(String(res.status));
      tokens = (await res.json()) as typeof tokens;
    } catch {
      throw new SsoError("SSO_EXCHANGE", "The identity provider didn't accept the sign-in.");
    }
    if (!tokens.id_token) throw new SsoError("SSO_TOKEN", "The identity provider sent no ID token.");
    let claims: Record<string, unknown>;
    try {
      const jwksRes = await this.get(doc.jwks_uri);
      const jwks = createLocalJWKSet((await jwksRes.json()) as JSONWebKeySet);
      ({ payload: claims } = await jwtVerify(tokens.id_token, jwks, {
        issuer: doc.issuer,
        audience: this.conn.clientId,
        clockTolerance: 60,
      }));
    } catch {
      throw new SsoError("SSO_TOKEN", "The identity provider's ID token didn't verify.");
    }
    if (claims.nonce !== input.nonce) throw new SsoError("SSO_TOKEN", "The ID token is for another sign-in.");
    let email = typeof claims.email === "string" ? claims.email : null;
    let verified = claims.email_verified;
    let name = typeof claims.name === "string" ? claims.name : null;
    if (!email && doc.userinfo_endpoint && tokens.access_token) {
      try {
        const res = await this.get(doc.userinfo_endpoint, { headers: { authorization: `Bearer ${tokens.access_token}` } });
        const info = (await res.json()) as Record<string, unknown>;
        if (info.sub === claims.sub) {
          email = typeof info.email === "string" ? info.email : null;
          verified = info.email_verified;
          name = name ?? (typeof info.name === "string" ? info.name : null);
        }
      } catch {
        // No email then; refused below.
      }
    }
    // An address the provider itself marks unverified is never used.
    if (!email || verified === false || verified === "false") {
      throw new SsoError("SSO_TOKEN", "The identity provider sent no verified email address.");
    }
    if (typeof claims.sub !== "string" || !claims.sub) throw new SsoError("SSO_TOKEN", "The ID token has no subject.");
    return { subject: claims.sub, email: email.trim().toLowerCase(), name };
  }
}
