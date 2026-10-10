/** One organization's identity provider (F-BL-06 P2). */
export type SsoConnection =
  | { provider: "oidc"; issuer: string; clientId: string; clientSecret: string }
  /** SAML (and OIDC) brokered by WorkOS; `organization` is the WorkOS organization id. */
  | { provider: "workos"; organization: string }
  /** Test-route identity provider; only with test routes enabled. */
  | { provider: "mock" };

/** Who the identity provider says signed in. */
export type SsoProfile = {
  /** The provider's stable id for the person. */
  subject: string;
  email: string;
  name: string | null;
};

export type AuthorizeInput = {
  state: string;
  nonce: string;
  codeChallenge: string;
  redirectUri: string;
  loginHint?: string;
};

export type ExchangeInput = { code: string; codeVerifier: string; nonce: string; redirectUri: string };

export interface SsoProvider {
  /** Checks the connection can be used (OIDC: the discovery document). */
  check(): Promise<void>;
  authorizeUrl(input: AuthorizeInput): Promise<string>;
  exchange(input: ExchangeInput): Promise<SsoProfile>;
}

/** A failed step, with the code the sign-in page explains. */
export class SsoError extends Error {
  constructor(
    readonly code: "SSO_CONFIG" | "SSO_EXCHANGE" | "SSO_TOKEN",
    message: string,
  ) {
    super(message);
  }
}

export type Fetch = (input: string, init?: RequestInit) => Promise<Response>;
