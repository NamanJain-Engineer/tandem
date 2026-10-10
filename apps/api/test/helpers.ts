import { generateKeyPairSync } from "node:crypto";
import { sql } from "drizzle-orm";
import { createApp } from "../src/app";
import { signAccessToken } from "../src/auth/jwt";
import type { KeySet } from "../src/auth/keys";
import { buildDeps } from "../src/buildDeps";
import type { Deps } from "../src/deps";
import { loadEnv } from "../src/env";
import { Clock } from "../src/lib/clock";
import { newId } from "../src/lib/ids";
import { MockBilling } from "../src/providers/billing/mock";
import { signPayload } from "../src/providers/billing/signature";
import { MemoryEmail } from "../src/providers/email/smtp";
import { subscriptions, user } from "../src/db/schema";

export const WEB = "http://localhost:3000";
export const PASSWORD = "Correct-horse-42";

function key(kid: string) {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  return { kid, privateKey, publicKey };
}

export function testKeys(): KeySet {
  return { access: [key("ak-1")], entitlement: [key("ek-1")], config: [key("ck-1")] };
}

export type TestEnv = Awaited<ReturnType<typeof makeTestApp>>;

export async function makeTestApp(envOverrides: Record<string, string> = {}) {
  const env = loadEnv({
    NODE_ENV: "test",
    DATABASE_URL: "postgres://tandem:tandem@localhost:5432/tandem_test",
    REDIS_URL: "redis://localhost:6379/1",
    WEB_PUBLIC_URL: WEB,
    API_PUBLIC_URL: "http://127.0.0.1:8787",
    TANDEM_API_TEST_ROUTES: "1",
    STRIPE_WEBHOOK_SECRET: "whsec_test",
    LOG_LEVEL: "silent",
    ...envOverrides,
  });
  const email = new MemoryEmail();
  const clock = new Clock();
  const keys = testKeys();
  const billing = new MockBilling(WEB, env.STRIPE_WEBHOOK_SECRET, clock);
  const built = buildDeps(env, { email, clock, keys, billing });
  const { deps } = built;
  await deps.redis.flushdb();
  await deps.db.execute(
    sql`TRUNCATE "user", session, account, verification, refresh_tokens, desktop_login_codes, subscriptions,
        billing_customers, webhook_events, credit_ledger, credit_reservations, credit_buckets, orgs, org_members,
        org_invites, org_policies, member_usage, daily_usage, ai_requests, audit_events, admin_actions RESTART IDENTITY CASCADE`,
  );
  const app = createApp(deps);
  return { app, deps, email, clock, billing, close: built.close };
}

/** A tiny cookie jar for app.request(). */
export class Jar {
  private cookies = new Map<string, string>();

  store(res: Response): void {
    for (const raw of res.headers.getSetCookie()) {
      const [pair] = raw.split(";");
      const [name, ...value] = (pair ?? "").split("=");
      if (!name) continue;
      const v = value.join("=");
      if (v === "" || /max-age=0/i.test(raw)) this.cookies.delete(name.trim());
      else this.cookies.set(name.trim(), v);
    }
  }

  get(name: string): string | undefined {
    return this.cookies.get(name);
  }

  header(): string {
    return [...this.cookies].map(([k, v]) => `${k}=${v}`).join("; ");
  }
}

export async function json(
  t: TestEnv,
  method: string,
  path: string,
  opts: { body?: unknown; jar?: Jar; bearer?: string; headers?: Record<string, string>; csrf?: boolean } = {},
): Promise<Response> {
  const headers: Record<string, string> = {
    "content-type": "application/json",
    origin: WEB,
    ...opts.headers,
  };
  if (opts.jar) {
    const cookie = opts.jar.header();
    if (cookie) headers.cookie = cookie;
    const token = opts.jar.get("tandem_csrf");
    if (opts.csrf !== false && token) headers["x-csrf-token"] = token;
  }
  if (opts.bearer) headers.authorization = `Bearer ${opts.bearer}`;
  const res = await t.app.request(path, {
    method,
    headers,
    body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
  });
  opts.jar?.store(res);
  return res;
}

/** Signs up, follows the verification link from the captured email, and signs in. */
export async function signUpVerified(
  t: TestEnv,
  email: string,
  password = PASSWORD,
): Promise<{ jar: Jar; userId: string }> {
  const jar = new Jar();
  await json(t, "GET", "/v1/health", { jar });
  const signup = await json(t, "POST", "/v1/auth/sign-up/email", {
    body: { email, password, name: "Test User" },
    jar,
  });
  if (signup.status !== 200) throw new Error(`sign-up failed: ${signup.status} ${await signup.text()}`);
  const mail = t.email.latest(email);
  const link = mail?.text.match(/https?:\/\/\S+/)?.[0];
  if (!link) throw new Error("no verification link");
  const url = new URL(link);
  const verify = await t.app.request(url.pathname + url.search, { headers: { cookie: jar.header() } });
  jar.store(verify);
  const signin = await json(t, "POST", "/v1/auth/sign-in/email", { body: { email, password }, jar });
  if (signin.status !== 200) throw new Error(`sign-in failed: ${signin.status} ${await signin.text()}`);
  const body = (await signin.json()) as { user: { id: string } };
  return { jar, userId: body.user.id };
}

export async function bearerFor(deps: Deps, userId: string): Promise<string> {
  return signAccessToken(deps.keys, { sub: userId, sid: newId("fam") }, deps.clock.nowSec());
}

export async function setSubscription(
  deps: Deps,
  userId: string,
  fields: Partial<typeof subscriptions.$inferInsert> = {},
): Promise<void> {
  const now = deps.clock.now();
  await deps.db.insert(subscriptions).values({
    id: newId("sub"),
    stripeSubscriptionId: newId("sub_test"),
    ownerKind: "user",
    ownerId: userId,
    plan: "pro",
    interval: "month",
    status: "active",
    seats: 1,
    currentPeriodStart: now,
    currentPeriodEnd: new Date(now.getTime() + 30 * 86_400_000),
    cancelAtPeriodEnd: false,
    updatedAt: now,
    ...fields,
  });
}

export function signed(t: TestEnv, event: unknown): { body: string; signature: string } {
  const body = JSON.stringify(event);
  return { body, signature: signPayload(body, "whsec_test", t.clock.nowSec()) };
}

export async function verifiedUser(t: TestEnv, email: string) {
  const { userId } = await signUpVerified(t, email);
  void user;
  return userId;
}
