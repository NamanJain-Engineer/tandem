import { eq } from "drizzle-orm";
import { Hono } from "hono";
import { exportJWK, generateKeyPair, SignJWT } from "jose";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { auditEvents, orgMembers, orgSso, user } from "../src/db/schema";
import { WorkOsSso } from "../src/providers/sso/workos";
import { membershipOf } from "../src/services/membership";
import { safeCallback, sealSecret } from "../src/services/sso";
import { json, makeTestApp, signUpVerified, WEB, type Jar, type TestEnv } from "./helpers";

const ISSUER = "https://idp.acme.test";
const CLIENT_ID = "vibezeo-client";
const CLIENT_SECRET = "s3cret-client-value-0123456789";

/** A small OpenID provider: discovery, keys, and a token endpoint for codes the test issues. */
class FakeIdp {
  codes = new Map<string, { nonce: string; email: string; sub: string; verified?: boolean }>();
  key!: CryptoKey;
  jwks: unknown;
  /** Sign with a key the JWKS doesn't have (a forged token). */
  forge = false;
  tokenCalls: Array<{ auth: string | null; form: URLSearchParams }> = [];
  app = new Hono();

  async init() {
    const { privateKey, publicKey } = await generateKeyPair("RS256");
    this.key = privateKey as CryptoKey;
    this.jwks = { keys: [{ ...(await exportJWK(publicKey)), kid: "k1", alg: "RS256", use: "sig" }] };
    this.app.get("/.well-known/openid-configuration", (c) =>
      c.json({
        issuer: ISSUER,
        authorization_endpoint: `${ISSUER}/authorize`,
        token_endpoint: `${ISSUER}/token`,
        jwks_uri: `${ISSUER}/jwks`,
        token_endpoint_auth_methods_supported: ["client_secret_basic"],
      }),
    );
    this.app.get("/jwks", (c) => c.json(this.jwks as object));
    this.app.post("/token", async (c) => {
      const form = new URLSearchParams(await c.req.text());
      this.tokenCalls.push({ auth: c.req.header("authorization") ?? null, form });
      const grant = this.codes.get(form.get("code") ?? "");
      if (!grant) return c.json({ error: "invalid_grant" }, 400);
      this.codes.delete(form.get("code") ?? "");
      const signer = this.forge ? (await generateKeyPair("RS256")).privateKey : this.key;
      const idToken = await new SignJWT({
        nonce: grant.nonce,
        email: grant.email,
        email_verified: grant.verified ?? true,
        name: "Ada Lovelace",
      })
        .setProtectedHeader({ alg: "RS256", kid: "k1" })
        .setIssuer(ISSUER)
        .setAudience(CLIENT_ID)
        .setSubject(grant.sub)
        .setIssuedAt()
        .setExpirationTime("5m")
        .sign(signer);
      return c.json({ id_token: idToken, access_token: "at", token_type: "Bearer" });
    });
  }

  fetch = async (input: string, init?: RequestInit): Promise<Response> => {
    if (!input.startsWith(ISSUER)) throw new Error(`no route to ${input}`);
    return this.app.request(input.slice(ISSUER.length), init);
  };
}

let t: TestEnv;
let idp: FakeIdp;
let txt: Record<string, string[][]> = {};

beforeAll(async () => {
  idp = new FakeIdp();
  await idp.init();
});
beforeEach(async () => {
  txt = {};
  idp.codes.clear();
  idp.forge = false;
  idp.tokenCalls = [];
  t = await makeTestApp({}, { ssoNet: { fetch: idp.fetch, resolveTxt: async (name) => txt[name] ?? [] } });
});
afterEach(async () => {
  await t.close();
});

/** An owner who bought Team (mock checkout) with `seats` seats. */
async function teamOwner(email: string, seats = 3) {
  const owner = await signUpVerified(t, email);
  const res = await json(t, "POST", "/v1/billing/checkout", { jar: owner.jar, body: { plan: "team", interval: "month", seats } });
  const sessionId = new URL(((await res.json()) as { url: string }).url).searchParams.get("session") ?? "";
  await json(t, "POST", "/v1/billing/mock/complete", { jar: owner.jar, body: { sessionId } });
  const orgId = (await membershipOf(t.deps.db, owner.userId))?.orgId ?? "";
  return { ...owner, orgId };
}

type View = {
  connection: { provider: string; hasSecret: boolean; enforce: boolean } | null;
  domains: Array<{ domain: string; verified: boolean; record: { name: string; value: string } }>;
};

async function configure(owner: { jar: Jar; orgId: string }, domain = "acme.test", enforce = false) {
  const put = await json(t, "PUT", `/v1/orgs/${owner.orgId}/sso`, {
    jar: owner.jar,
    body: { provider: "oidc", issuer: ISSUER, clientId: CLIENT_ID, clientSecret: CLIENT_SECRET },
  });
  expect(put.status).toBe(200);
  const add = await json(t, "POST", `/v1/orgs/${owner.orgId}/sso/domains`, { jar: owner.jar, body: { domain } });
  expect(add.status).toBe(201);
  const record = ((await add.json()) as View).domains.find((d) => d.domain === domain)?.record;
  txt[record?.name ?? ""] = [[record?.value ?? ""]];
  const ok = await json(t, "POST", `/v1/orgs/${owner.orgId}/sso/domains/${domain}/verify`, { jar: owner.jar });
  expect(ok.status).toBe(200);
  if (enforce) {
    const on = await json(t, "PUT", `/v1/orgs/${owner.orgId}/sso`, {
      jar: owner.jar,
      body: { provider: "oidc", issuer: ISSUER, clientId: CLIENT_ID, enforce: true },
    });
    expect(on.status).toBe(200);
  }
}

/** Starts at `/v1/sso/start`, lets the fake IdP "sign in" `as`, and follows the callback. */
async function signInWithSso(email: string, as: { email?: string; sub?: string; verified?: boolean } = {}, callbackURL?: string) {
  const start = await json(t, "POST", "/v1/sso/start", { body: { email, callbackURL } });
  expect(start.status).toBe(200);
  const url = new URL(((await start.json()) as { url: string }).url);
  const state = url.searchParams.get("state") ?? "";
  const nonce = url.searchParams.get("nonce") ?? "";
  const code = `code-${Math.random()}`;
  idp.codes.set(code, { nonce, email: as.email ?? email, sub: as.sub ?? `sub-${email}`, verified: as.verified });
  const res = await t.app.request(`/v1/auth/sso/callback?state=${state}&code=${code}`, { headers: { origin: WEB } });
  return { res, url, state, code };
}

const cookieOf = (res: Response) => res.headers.get("set-cookie") ?? "";

describe("F-BL-06 P2 Team SSO: setting it up", () => {
  it("an admin saves an OIDC connection after its discovery document checks out; the secret is sealed", async () => {
    const owner = await teamOwner("own@acme.test");
    await configure(owner);
    const view = (await (await json(t, "GET", `/v1/orgs/${owner.orgId}/sso`, { jar: owner.jar })).json()) as View;
    expect(view.connection).toMatchObject({ provider: "oidc", hasSecret: true, enforce: false });
    expect(JSON.stringify(view)).not.toContain(CLIENT_SECRET);
    const [row] = await t.deps.db.select().from(orgSso).where(eq(orgSso.orgId, owner.orgId));
    expect(row?.clientSecretEnc).toBeTruthy();
    expect(row?.clientSecretEnc).not.toContain(CLIENT_SECRET);
    expect(sealSecret(t.deps, "x")).not.toBe(sealSecret(t.deps, "x"));
    const events = (await t.deps.db.select().from(auditEvents).where(eq(auditEvents.orgId, owner.orgId))).map((e) => e.event);
    expect(events).toEqual(expect.arrayContaining(["sso_changed", "sso_domain_added", "sso_domain_verified"]));
  });

  it("an issuer whose discovery document can't be read is refused", async () => {
    const owner = await teamOwner("own2@acme.test");
    const res = await json(t, "PUT", `/v1/orgs/${owner.orgId}/sso`, {
      jar: owner.jar,
      body: { provider: "oidc", issuer: "https://nowhere.test", clientId: "x", clientSecret: "y" },
    });
    expect(res.status).toBe(422);
    expect(await res.json()).toMatchObject({ code: "SSO_CONFIG" });
  });

  it("a domain counts only once its TXT record is there, and only for one org", async () => {
    const owner = await teamOwner("own3@acme.test");
    const add = await json(t, "POST", `/v1/orgs/${owner.orgId}/sso/domains`, { jar: owner.jar, body: { domain: "Acme.test" } });
    const record = ((await add.json()) as View).domains[0]?.record;
    expect(record?.name).toBe("_vibezeo-sso.acme.test");
    expect(record?.value).toMatch(/^vibezeo-sso=/);
    const early = await json(t, "POST", `/v1/orgs/${owner.orgId}/sso/domains/acme.test/verify`, { jar: owner.jar });
    expect(early.status).toBe(422);
    expect(await early.json()).toMatchObject({ code: "DOMAIN_UNVERIFIED" });
    // Requiring SSO needs a verified domain.
    const enforce = await json(t, "PUT", `/v1/orgs/${owner.orgId}/sso`, {
      jar: owner.jar,
      body: { provider: "oidc", issuer: ISSUER, clientId: CLIENT_ID, clientSecret: CLIENT_SECRET, enforce: true },
    });
    expect(enforce.status).toBe(422);
    expect(await enforce.json()).toMatchObject({ code: "SSO_NO_DOMAIN" });
    txt[record?.name ?? ""] = [["v=spf1 -all"], [record?.value ?? ""]];
    const ok = await json(t, "POST", `/v1/orgs/${owner.orgId}/sso/domains/acme.test/verify`, { jar: owner.jar });
    expect(((await ok.json()) as View).domains[0]?.verified).toBe(true);
    // Another org can't claim it now.
    const other = await teamOwner("boss@other.test");
    const steal = await json(t, "POST", `/v1/orgs/${other.orgId}/sso/domains`, { jar: other.jar, body: { domain: "acme.test" } });
    expect(steal.status).toBe(409);
  });

  it("members can't change SSO, and orgs not on a paid Team can't set it up", async () => {
    const owner = await teamOwner("own4@acme.test");
    const member = await signUpVerified(t, "mem4@acme.test");
    await t.deps.db.insert(orgMembers).values({ orgId: owner.orgId, userId: member.userId, role: "member" });
    const res = await json(t, "PUT", `/v1/orgs/${owner.orgId}/sso`, {
      jar: member.jar,
      body: { provider: "oidc", issuer: ISSUER, clientId: CLIENT_ID, clientSecret: CLIENT_SECRET },
    });
    expect(res.status).toBe(403);
    const mock = await json(t, "PUT", `/v1/orgs/${owner.orgId}/sso`, { jar: owner.jar, body: { provider: "workos", organization: "org_1" } });
    expect(mock.status).toBe(422);
    expect(await mock.json()).toMatchObject({ code: "SSO_UNAVAILABLE" });
  });
});

describe("F-BL-06 P2 Team SSO: signing in", () => {
  it("OIDC sign-in creates the account, joins the org and lands signed in", async () => {
    const owner = await teamOwner("own5@acme.test");
    await configure(owner);
    const { res, url } = await signInWithSso("Ada@Acme.test", {}, "/team");
    expect(url.origin + url.pathname).toBe(`${ISSUER}/authorize`);
    expect(url.searchParams.get("code_challenge_method")).toBe("S256");
    expect(url.searchParams.get("redirect_uri")).toBe(`${WEB}/v1/auth/sso/callback`);
    expect(res.status).toBe(302);
    expect(res.headers.get("location")).toBe(`${WEB}/team`);
    expect(cookieOf(res)).toContain("tandem.session_token=");
    // The token request authenticated as the client and proved the PKCE verifier.
    expect(idp.tokenCalls[0]?.auth).toMatch(/^Basic /);
    expect(idp.tokenCalls[0]?.form.get("code_verifier")).toBeTruthy();
    const me = await t.app.request("/v1/me", { headers: { cookie: cookieOf(res).split(";")[0] ?? "", origin: WEB } });
    expect(me.status).toBe(200);
    expect(await me.json()).toMatchObject({ user: { email: "ada@acme.test", emailVerified: true }, plan: "team", org: { id: owner.orgId, role: "member" } });
    const events = (await t.deps.db.select().from(auditEvents).where(eq(auditEvents.orgId, owner.orgId))).map((e) => e.event);
    expect(events).toEqual(expect.arrayContaining(["member_added", "sso_sign_in"]));
  });

  it("an existing account at the domain signs in to itself and joins", async () => {
    const owner = await teamOwner("own6@acme.test");
    await configure(owner);
    const bob = await signUpVerified(t, "bob@acme.test");
    const { res } = await signInWithSso("bob@acme.test");
    expect(res.status).toBe(302);
    expect((await membershipOf(t.deps.db, bob.userId))?.orgId).toBe(owner.orgId);
    expect((await t.deps.db.select().from(user).where(eq(user.email, "bob@acme.test"))).length).toBe(1);
  });

  it("the provider can't sign in an address outside the org's verified domains", async () => {
    const owner = await teamOwner("own7@acme.test");
    await configure(owner);
    const victim = await signUpVerified(t, "victim@elsewhere.test");
    const { res } = await signInWithSso("eve@acme.test", { email: "victim@elsewhere.test" });
    expect(res.headers.get("location")).toBe(`${WEB}/signin?sso_error=SSO_DOMAIN`);
    expect(cookieOf(res)).not.toContain("session_token=");
    expect(await membershipOf(t.deps.db, victim.userId)).toBeNull();
  });

  it("a forged ID token, an unverified address, a reused state or a denial all fail closed", async () => {
    const owner = await teamOwner("own8@acme.test");
    await configure(owner);
    idp.forge = true;
    expect((await signInWithSso("f@acme.test")).res.headers.get("location")).toContain("sso_error=SSO_TOKEN");
    idp.forge = false;
    expect((await signInWithSso("u@acme.test", { verified: false })).res.headers.get("location")).toContain("sso_error=SSO_TOKEN");
    const first = await signInWithSso("r@acme.test");
    expect(first.res.headers.get("location")).toBe(`${WEB}/account`);
    idp.codes.set("again", { nonce: "n", email: "r@acme.test", sub: "s" });
    const replay = await t.app.request(`/v1/auth/sso/callback?state=${first.state}&code=again`);
    expect(replay.headers.get("location")).toContain("sso_error=SSO_STATE");
    const start = await json(t, "POST", "/v1/sso/start", { body: { email: "d@acme.test" } });
    const state = new URL(((await start.json()) as { url: string }).url).searchParams.get("state");
    const denied = await t.app.request(`/v1/auth/sso/callback?state=${state}&error=access_denied`);
    expect(denied.headers.get("location")).toContain("sso_error=SSO_DENIED");
  });

  it("no seat left or another org already: refused with the reason", async () => {
    const owner = await teamOwner("own9@acme.test", 2);
    await configure(owner);
    expect((await signInWithSso("one@acme.test")).res.headers.get("location")).toBe(`${WEB}/account`);
    expect((await signInWithSso("two@acme.test")).res.headers.get("location")).toContain("sso_error=SEATS_FULL");
    const other = await teamOwner("lead@acme-two.test");
    await t.deps.db.update(user).set({ email: "lead@acme.test" }).where(eq(user.id, other.userId));
    expect((await signInWithSso("lead@acme.test")).res.headers.get("location")).toContain("sso_error=SSO_OTHER_ORG");
  });

  it("an address without SSO gets 404 NO_SSO, and the landing page stays on the web app", async () => {
    const res = await json(t, "POST", "/v1/sso/start", { body: { email: "x@nosso.test" } });
    expect(res.status).toBe(404);
    expect(await res.json()).toMatchObject({ code: "NO_SSO" });
    expect(safeCallback("//evil.test/x")).toBe("/account");
    expect(safeCallback("https://evil.test")).toBe("/account");
    expect(safeCallback("/\\evil.test")).toBe("/account");
    expect(safeCallback("/team?x=1")).toBe("/team?x=1");
  });

  it("with SSO required, passwords and magic links stop for the domain only", async () => {
    const owner = await teamOwner("own10@acme.test");
    const bob = await signUpVerified(t, "carol@acme.test");
    const outsider = await signUpVerified(t, "dan@other.test");
    await configure(owner, "acme.test", true);
    const lookup = await json(t, "POST", "/v1/sso/lookup", { body: { email: "carol@acme.test" } });
    expect(await lookup.json()).toMatchObject({ sso: true, required: true });
    const pw = await json(t, "POST", "/v1/auth/sign-in/email", { body: { email: "carol@acme.test", password: "Correct-horse-42" } });
    expect(pw.status).toBe(403);
    expect(await pw.json()).toMatchObject({ code: "SSO_REQUIRED" });
    const magic = await json(t, "POST", "/v1/auth/sign-in/magic-link", { body: { email: "carol@acme.test" } });
    expect(magic.status).toBe(403);
    const ok = await json(t, "POST", "/v1/auth/sign-in/email", { body: { email: "dan@other.test", password: "Correct-horse-42" } });
    expect(ok.status).toBe(200);
    // SSO itself still works for the domain.
    expect((await signInWithSso("carol@acme.test")).res.headers.get("location")).toBe(`${WEB}/account`);
    expect(bob.userId).not.toBe(outsider.userId);
  });
});

describe("F-BL-06 P2 SAML through WorkOS", () => {
  const calls: Array<{ url: string; body: string }> = [];
  const fakeFetch =
    (profile: Record<string, unknown>) =>
    async (url: string, init?: RequestInit): Promise<Response> => {
      calls.push({ url, body: String(init?.body ?? "") });
      return new Response(JSON.stringify({ profile, access_token: "x" }), { status: 200 });
    };

  it("sends the browser to WorkOS for the org and takes the profile for its organization only", async () => {
    const sso = new WorkOsSso({ organization: "org_01ACME" }, { clientId: "client_1", apiKey: "sk_test" }, fakeFetch({
      id: "prof_1",
      email: "Ada@Acme.test",
      first_name: "Ada",
      last_name: "Lovelace",
      organization_id: "org_01ACME",
    }));
    const url = new URL(await sso.authorizeUrl({ state: "s", nonce: "n", codeChallenge: "c", redirectUri: "https://web/cb" }));
    expect(url.origin + url.pathname).toBe("https://api.workos.com/sso/authorize");
    expect(Object.fromEntries(url.searchParams)).toMatchObject({ organization: "org_01ACME", client_id: "client_1", state: "s" });
    expect(await sso.exchange({ code: "c1", codeVerifier: "v", nonce: "n", redirectUri: "https://web/cb" })).toEqual({
      subject: "prof_1",
      email: "ada@acme.test",
      name: "Ada Lovelace",
    });
    expect(calls.at(-1)?.body).toContain("grant_type=authorization_code");
    const other = new WorkOsSso({ organization: "org_01ACME" }, { clientId: "c", apiKey: "k" }, fakeFetch({
      id: "p",
      email: "x@evil.test",
      organization_id: "org_02EVIL",
    }));
    await expect(other.exchange({ code: "c", codeVerifier: "v", nonce: "n", redirectUri: "r" })).rejects.toThrow(/another organization/);
  });
});
