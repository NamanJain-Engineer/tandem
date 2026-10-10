// Team single sign-on (F-BL-06 P2, ARCHITECTURE §14.7): an org's identity
// provider (OIDC, or SAML through WorkOS), the email domains it proves with a
// DNS TXT record, sign-in that joins the org, and "SSO only" for its domains.
import { createCipheriv, createDecipheriv, createHash, hkdfSync, randomBytes } from "node:crypto";
import { and, eq, isNotNull } from "drizzle-orm";
import { z } from "zod";
import type { Deps } from "../deps";
import { orgMembers, orgSso, orgSsoDomains, orgs } from "../db/schema";
import { testRoutesEnabled } from "../env";
import { randomToken } from "../lib/ids";
import { logger } from "../lib/logger";
import { ApiError } from "../lib/problem";
import { MockSso } from "../providers/sso/mock";
import { OidcSso } from "../providers/sso/oidc";
import { SsoError, type SsoConnection, type SsoProfile, type SsoProvider } from "../providers/sso/types";
import { WorkOsSso, WORKOS_API } from "../providers/sso/workos";
import { membershipOf } from "./membership";
import { audit, orgPlan, roleIn, seatsInUse } from "./orgs";

/** The TXT record that proves a domain: `_vibezeo-sso.<domain>` = `vibezeo-sso=<token>`. */
export const TXT_PREFIX = "_vibezeo-sso";
const STATE_TTL_MS = 10 * 60_000;
const stateKey = (s: string) => `sso:state:${s}`;

export const ssoBody = z.discriminatedUnion("provider", [
  z.object({
    provider: z.literal("oidc"),
    issuer: z.string().url().max(500),
    clientId: z.string().min(1).max(300),
    /** Omit to keep the stored secret. */
    clientSecret: z.string().min(1).max(2000).optional(),
    enforce: z.boolean().default(false),
  }),
  z.object({
    provider: z.literal("workos"),
    organization: z.string().min(1).max(100),
    enforce: z.boolean().default(false),
  }),
  z.object({ provider: z.literal("mock"), enforce: z.boolean().default(false) }),
]);
export type SsoBody = z.infer<typeof ssoBody>;

export const domainBody = z.object({
  domain: z
    .string()
    .trim()
    .toLowerCase()
    .max(253)
    .regex(/^(?=.{1,253}$)([a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$/, "Enter a domain like example.com."),
});

// ── the client secret, encrypted at rest (D-626) ──────────────────────────
function secretKey(deps: Deps): Buffer {
  return Buffer.from(hkdfSync("sha256", deps.env.BETTER_AUTH_SECRET, "vibezeo-sso", "client-secret", 32));
}

export function sealSecret(deps: Deps, plain: string): string {
  const iv = randomBytes(12);
  const cipher = createCipheriv("aes-256-gcm", secretKey(deps), iv);
  const body = Buffer.concat([cipher.update(plain, "utf8"), cipher.final()]);
  return Buffer.concat([iv, cipher.getAuthTag(), body]).toString("base64");
}

function openSecret(deps: Deps, sealed: string): string {
  const raw = Buffer.from(sealed, "base64");
  const decipher = createDecipheriv("aes-256-gcm", secretKey(deps), raw.subarray(0, 12));
  decipher.setAuthTag(raw.subarray(12, 28));
  return Buffer.concat([decipher.update(raw.subarray(28)), decipher.final()]).toString("utf8");
}

// ── providers ─────────────────────────────────────────────────────────────
export function workosAvailable(deps: Deps): boolean {
  return Boolean(deps.env.WORKOS_CLIENT_ID && deps.env.WORKOS_API_KEY);
}

export function providerFor(deps: Deps, conn: SsoConnection): SsoProvider {
  switch (conn.provider) {
    case "oidc":
      return new OidcSso(conn, deps.ssoNet.fetch);
    case "workos":
      return new WorkOsSso(
        conn,
        { clientId: deps.env.WORKOS_CLIENT_ID ?? "", apiKey: deps.env.WORKOS_API_KEY ?? "" },
        deps.ssoNet.fetch,
        deps.env.WORKOS_API_BASE ?? WORKOS_API,
      );
    case "mock":
      return new MockSso(deps.redis, deps.env.API_PUBLIC_URL);
  }
}

type SsoRow = typeof orgSso.$inferSelect;

function connectionOf(deps: Deps, row: SsoRow): SsoConnection | null {
  if (row.provider === "oidc" && row.issuer && row.clientId && row.clientSecretEnc) {
    return { provider: "oidc", issuer: row.issuer, clientId: row.clientId, clientSecret: openSecret(deps, row.clientSecretEnc) };
  }
  if (row.provider === "workos" && row.organization && workosAvailable(deps)) {
    return { provider: "workos", organization: row.organization };
  }
  if (row.provider === "mock" && testRoutesEnabled(deps.env)) return { provider: "mock" };
  return null;
}

// ── admin: the connection and its domains ─────────────────────────────────
async function requireTeam(deps: Deps, orgId: string) {
  const { effective } = await orgPlan(deps, orgId);
  if (effective.plan !== "team") {
    throw new ApiError(403, "FORBIDDEN_PLAN", "Forbidden", "Single sign-on is part of the Team plan.", {
      feature: "sso",
    });
  }
}

export async function ssoView(deps: Deps, orgId: string, viewerId: string) {
  await roleIn(deps, orgId, viewerId, "admin");
  const [row] = await deps.db.select().from(orgSso).where(eq(orgSso.orgId, orgId)).limit(1);
  const domains = await deps.db
    .select()
    .from(orgSsoDomains)
    .where(eq(orgSsoDomains.orgId, orgId))
    .orderBy(orgSsoDomains.domain);
  return {
    connection: row
      ? {
          provider: row.provider,
          issuer: row.issuer,
          clientId: row.clientId,
          hasSecret: row.clientSecretEnc !== null,
          organization: row.organization,
          enforce: row.enforce,
          updatedAt: row.updatedAt.toISOString(),
        }
      : null,
    domains: domains.map((d) => ({
      domain: d.domain,
      verified: d.verifiedAt !== null,
      record: { type: "TXT", name: `${TXT_PREFIX}.${d.domain}`, value: `vibezeo-sso=${d.verifyToken}` },
    })),
    providers: { oidc: true, workos: workosAvailable(deps) },
    redirectUri: redirectUri(deps),
  };
}

export async function setSso(deps: Deps, orgId: string, actorId: string, body: SsoBody) {
  await roleIn(deps, orgId, actorId, "admin");
  await requireTeam(deps, orgId);
  const [current] = await deps.db.select().from(orgSso).where(eq(orgSso.orgId, orgId)).limit(1);
  let values: Omit<typeof orgSso.$inferInsert, "orgId" | "updatedAt">;
  if (body.provider === "oidc") {
    const secret =
      body.clientSecret ??
      (current?.provider === "oidc" && current.clientSecretEnc ? openSecret(deps, current.clientSecretEnc) : null);
    if (!secret) throw new ApiError(422, "VALIDATION", "Unprocessable Entity", "Enter the client secret.");
    await check(providerFor(deps, { provider: "oidc", issuer: body.issuer, clientId: body.clientId, clientSecret: secret }));
    values = {
      provider: "oidc",
      issuer: body.issuer.replace(/\/+$/, ""),
      clientId: body.clientId,
      clientSecretEnc: sealSecret(deps, secret),
      organization: null,
      enforce: body.enforce,
    };
  } else if (body.provider === "workos") {
    if (!workosAvailable(deps)) {
      throw new ApiError(422, "SSO_UNAVAILABLE", "Unprocessable Entity", "SAML through WorkOS isn't set up on this server yet.");
    }
    await check(providerFor(deps, { provider: "workos", organization: body.organization }));
    values = { provider: "workos", issuer: null, clientId: null, clientSecretEnc: null, organization: body.organization, enforce: body.enforce };
  } else {
    if (!testRoutesEnabled(deps.env)) {
      throw new ApiError(422, "VALIDATION", "Unprocessable Entity", "Choose OIDC or SAML.");
    }
    values = { provider: "mock", issuer: null, clientId: null, clientSecretEnc: null, organization: null, enforce: body.enforce };
  }
  if (values.enforce && !(await hasVerifiedDomain(deps, orgId))) {
    throw new ApiError(
      422,
      "SSO_NO_DOMAIN",
      "Unprocessable Entity",
      "Verify an email domain before requiring single sign-on.",
    );
  }
  const now = deps.clock.now();
  await deps.db
    .insert(orgSso)
    .values({ orgId, ...values, updatedAt: now })
    .onConflictDoUpdate({ target: orgSso.orgId, set: { ...values, updatedAt: now } });
  await audit(deps, orgId, actorId, "sso_changed", { provider: values.provider, enforce: values.enforce ?? false });
  return ssoView(deps, orgId, actorId);
}

async function check(provider: SsoProvider) {
  try {
    await provider.check();
  } catch (err) {
    if (err instanceof SsoError) throw new ApiError(422, err.code, "Unprocessable Entity", err.message);
    throw err;
  }
}

export async function removeSso(deps: Deps, orgId: string, actorId: string) {
  await roleIn(deps, orgId, actorId, "admin");
  await deps.db.delete(orgSso).where(eq(orgSso.orgId, orgId));
  await audit(deps, orgId, actorId, "sso_removed");
}

async function hasVerifiedDomain(deps: Deps, orgId: string): Promise<boolean> {
  const [d] = await deps.db
    .select({ domain: orgSsoDomains.domain })
    .from(orgSsoDomains)
    .where(and(eq(orgSsoDomains.orgId, orgId), isNotNull(orgSsoDomains.verifiedAt)))
    .limit(1);
  return d !== undefined;
}

export async function addDomain(deps: Deps, orgId: string, actorId: string, domain: string) {
  await roleIn(deps, orgId, actorId, "admin");
  await requireTeam(deps, orgId);
  const [taken] = await deps.db
    .select({ orgId: orgSsoDomains.orgId })
    .from(orgSsoDomains)
    .where(and(eq(orgSsoDomains.domain, domain), isNotNull(orgSsoDomains.verifiedAt)))
    .limit(1);
  if (taken && taken.orgId !== orgId) {
    throw new ApiError(409, "CONFLICT", "Conflict", "Another organization has verified that domain.");
  }
  await deps.db
    .insert(orgSsoDomains)
    .values({ orgId, domain, verifyToken: randomToken(16) })
    .onConflictDoNothing();
  await audit(deps, orgId, actorId, "sso_domain_added", { domain });
  return ssoView(deps, orgId, actorId);
}

/** Looks the TXT record up and marks the domain verified when it matches. */
export async function verifyDomain(deps: Deps, orgId: string, actorId: string, domain: string) {
  await roleIn(deps, orgId, actorId, "admin");
  const [row] = await deps.db
    .select()
    .from(orgSsoDomains)
    .where(and(eq(orgSsoDomains.orgId, orgId), eq(orgSsoDomains.domain, domain)))
    .limit(1);
  if (!row) throw new ApiError(404, "NOT_FOUND", "Not Found", "Add the domain first.");
  if (!row.verifiedAt) {
    const want = `vibezeo-sso=${row.verifyToken}`;
    const records = await deps.ssoNet.resolveTxt(`${TXT_PREFIX}.${domain}`).catch(() => [] as string[][]);
    if (!records.some((chunks) => chunks.join("") === want)) {
      throw new ApiError(
        422,
        "DOMAIN_UNVERIFIED",
        "Unprocessable Entity",
        `No TXT record ${TXT_PREFIX}.${domain} with ${want} yet. DNS changes can take a while to show.`,
      );
    }
    await markVerified(deps, orgId, domain);
    await audit(deps, orgId, actorId, "sso_domain_verified", { domain });
  }
  return ssoView(deps, orgId, actorId);
}

/** Marks a domain verified (after the DNS check, or from a test route). */
export async function markVerified(deps: Deps, orgId: string, domain: string) {
  try {
    await deps.db
      .update(orgSsoDomains)
      .set({ verifiedAt: deps.clock.now() })
      .where(and(eq(orgSsoDomains.orgId, orgId), eq(orgSsoDomains.domain, domain)));
  } catch {
    // The partial unique index: someone verified it first.
    throw new ApiError(409, "CONFLICT", "Conflict", "Another organization has verified that domain.");
  }
}

export async function removeDomain(deps: Deps, orgId: string, actorId: string, domain: string) {
  await roleIn(deps, orgId, actorId, "admin");
  await deps.db.delete(orgSsoDomains).where(and(eq(orgSsoDomains.orgId, orgId), eq(orgSsoDomains.domain, domain)));
  // Without a verified domain, "SSO only" would lock no one in and no one out: turn it off.
  if (!(await hasVerifiedDomain(deps, orgId))) {
    await deps.db.update(orgSso).set({ enforce: false }).where(eq(orgSso.orgId, orgId));
  }
  await audit(deps, orgId, actorId, "sso_domain_removed", { domain });
}

// ── sign-in ───────────────────────────────────────────────────────────────
const domainOf = (email: string) => email.trim().toLowerCase().split("@")[1] ?? "";

/** The org (with a usable connection) that owns this address's verified domain. */
async function orgForEmail(deps: Deps, email: string): Promise<{ orgId: string; row: SsoRow } | null> {
  const domain = domainOf(email);
  if (!domain) return null;
  const [hit] = await deps.db
    .select({ row: orgSso })
    .from(orgSsoDomains)
    .innerJoin(orgSso, eq(orgSso.orgId, orgSsoDomains.orgId))
    .where(and(eq(orgSsoDomains.domain, domain), isNotNull(orgSsoDomains.verifiedAt)))
    .limit(1);
  return hit ? { orgId: hit.row.orgId, row: hit.row } : null;
}

/** True when this address must sign in through its org's identity provider. */
export async function ssoRequiredFor(deps: Deps, email: string): Promise<boolean> {
  const hit = await orgForEmail(deps, email);
  if (!hit?.row.enforce) return false;
  const { effective } = await orgPlan(deps, hit.orgId);
  return effective.plan === "team";
}

export function redirectUri(deps: Deps): string {
  return `${deps.env.WEB_PUBLIC_URL}/v1/auth/sso/callback`;
}

/** Only paths on the web origin, never another site. */
export function safeCallback(raw: string | undefined): string {
  if (!raw || !raw.startsWith("/") || raw.startsWith("//") || raw.includes("\\")) return "/account";
  return raw;
}

const b64url = (b: Buffer) => b.toString("base64url");

/** `POST /v1/sso/start`: where to send the browser for this address. */
export async function startSso(deps: Deps, email: string, callback?: string): Promise<{ url: string }> {
  const hit = await orgForEmail(deps, email);
  const conn = hit ? connectionOf(deps, hit.row) : null;
  if (!hit || !conn || (await orgPlan(deps, hit.orgId)).effective.plan !== "team") {
    throw new ApiError(404, "NO_SSO", "Not Found", "Single sign-on isn't set up for that address. Sign in another way.");
  }
  const state = randomToken();
  const nonce = randomToken();
  const verifier = b64url(randomBytes(32));
  const challenge = b64url(createHash("sha256").update(verifier).digest());
  await deps.redis.set(
    stateKey(state),
    JSON.stringify({ orgId: hit.orgId, nonce, verifier, callback: safeCallback(callback) }),
    "PX",
    STATE_TTL_MS,
  );
  try {
    const url = await providerFor(deps, conn).authorizeUrl({
      state,
      nonce,
      codeChallenge: challenge,
      redirectUri: redirectUri(deps),
      loginHint: email.trim().toLowerCase(),
    });
    return { url };
  } catch (err) {
    if (err instanceof SsoError) throw new ApiError(502, err.code, "Bad Gateway", err.message);
    throw err;
  }
}

export type SsoFinish =
  | { ok: true; orgId: string; provider: SsoConnection["provider"]; profile: SsoProfile; callback: string }
  | { ok: false; code: string };

/** The callback: one-time state, the code exchanged, the address held to the org's domains. */
export async function finishSso(deps: Deps, q: { state?: string; code?: string; error?: string }): Promise<SsoFinish> {
  if (!q.state) return { ok: false, code: "SSO_STATE" };
  const raw = await deps.redis.getdel(stateKey(q.state));
  if (!raw) return { ok: false, code: "SSO_STATE" };
  const st = JSON.parse(raw) as { orgId: string; nonce: string; verifier: string; callback: string };
  if (q.error || !q.code) return { ok: false, code: "SSO_DENIED" };
  const [row] = await deps.db.select().from(orgSso).where(eq(orgSso.orgId, st.orgId)).limit(1);
  const conn = row ? connectionOf(deps, row) : null;
  if (!conn) return { ok: false, code: "NO_SSO" };
  let profile: SsoProfile;
  try {
    profile = await providerFor(deps, conn).exchange({
      code: q.code,
      codeVerifier: st.verifier,
      nonce: st.nonce,
      redirectUri: redirectUri(deps),
    });
  } catch (err) {
    logger.warn({ org: st.orgId, code: err instanceof SsoError ? err.code : "error" }, "sso exchange failed");
    return { ok: false, code: err instanceof SsoError ? err.code : "SSO_EXCHANGE" };
  }
  // The provider can only sign in addresses on the org's verified domains (D-627).
  const owner = await orgForEmail(deps, profile.email);
  if (owner?.orgId !== st.orgId) return { ok: false, code: "SSO_DOMAIN" };
  return { ok: true, orgId: st.orgId, provider: conn.provider, profile, callback: st.callback };
}

/** Joins the org on first sign-in (just-in-time), seats allowing. */
export async function admit(deps: Deps, orgId: string, userId: string): Promise<{ ok: true } | { ok: false; code: string }> {
  const member = await membershipOf(deps.db, userId);
  if (member) return member.orgId === orgId ? { ok: true } : { ok: false, code: "SSO_OTHER_ORG" };
  const { sub } = await orgPlan(deps, orgId);
  if ((await seatsInUse(deps, orgId)) >= (sub?.seats ?? 0)) return { ok: false, code: "SEATS_FULL" };
  await deps.db.insert(orgMembers).values({ orgId, userId, role: "member" }).onConflictDoNothing();
  await audit(deps, orgId, userId, "member_added", { role: "member", via: "sso" });
  await deps.events.publish(userId, "entitlement");
  return { ok: true };
}

export async function recordSignIn(deps: Deps, orgId: string, userId: string) {
  const [row] = await deps.db.select({ provider: orgSso.provider }).from(orgSso).where(eq(orgSso.orgId, orgId)).limit(1);
  await audit(deps, orgId, userId, "sso_sign_in", { provider: row?.provider ?? null });
}

/** For the sign-in page: does this address use SSO, and must it? */
export async function ssoLookup(deps: Deps, email: string) {
  const hit = await orgForEmail(deps, email);
  if (!hit || !connectionOf(deps, hit.row) || (await orgPlan(deps, hit.orgId)).effective.plan !== "team") {
    return { sso: false, required: false };
  }
  const [org] = await deps.db.select({ name: orgs.name }).from(orgs).where(eq(orgs.id, hit.orgId)).limit(1);
  return { sso: true, required: hit.row.enforce, org: org?.name ?? null };
}

