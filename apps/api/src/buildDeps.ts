import { Redis } from "ioredis";
import { fileURLToPath } from "node:url";
import { createAuth } from "./auth/better-auth";
import { loadKeys } from "./auth/keys";
import { createDb } from "./db/client";
import { resolveTxt } from "node:dns/promises";
import type { Deps } from "./deps";
import type { Env } from "./env";
import { Clock } from "./lib/clock";
import { selectAi } from "./providers/ai";
import { selectStt } from "./providers/stt";
import { MockBilling } from "./providers/billing/mock";
import { StripeBilling } from "./providers/billing/stripe";
import { HibpBreachChecker, OfflineBreachList, type BreachChecker } from "./providers/breach";
import { DevCaptcha, TurnstileCaptcha } from "./providers/captcha";
import { SmtpEmail } from "./providers/email/smtp";
import type { EmailProvider } from "./providers/email/types";
import { ConfigSigner } from "./services/configSigner";
import { EventHub } from "./services/events";
import { RelayControl } from "./services/relayControl";

export type Built = { deps: Deps; close: () => Promise<void> };

const repoRoot = fileURLToPath(new URL("../../..", import.meta.url));

/** Wires real providers from the environment. Mocks are chosen when keys are absent. */
export function buildDeps(
  env: Env,
  overrides: Partial<Pick<Deps, "email" | "clock" | "keys" | "billing" | "breach" | "ai" | "stt" | "ssoNet">> = {},
): Built {
  const { db, pool } = createDb(env.DATABASE_URL);
  const redis = new Redis(env.REDIS_URL, { lazyConnect: false, maxRetriesPerRequest: 2 });
  const sub = new Redis(env.REDIS_URL, { lazyConnect: false });
  const clock = overrides.clock ?? new Clock();
  const keys =
    overrides.keys ??
    loadKeys({
      jwtKeys: env.JWT_SIGNING_KEYS,
      configKey: env.CONFIG_SIGNING_KEY,
      repoRoot,
      allowGenerate: env.NODE_ENV !== "production",
    });
  const email: EmailProvider = overrides.email ?? new SmtpEmail(env.SMTP_URL, env.EMAIL_FROM);
  const breach: BreachChecker =
    overrides.breach ?? (env.BREACH_CHECK === "hibp" ? new HibpBreachChecker() : new OfflineBreachList());
  const billing =
    overrides.billing ??
    (env.BILLING_PROVIDER === "stripe"
      ? new StripeBilling(env.STRIPE_SECRET_KEY, env.STRIPE_WEBHOOK_SECRET, env.STRIPE_API_BASE)
      : new MockBilling(env.WEB_PUBLIC_URL, env.STRIPE_WEBHOOK_SECRET, clock));
  const deps: Deps = {
    env,
    db,
    redis,
    clock,
    keys,
    email,
    breach,
    captcha: env.TURNSTILE_SECRET ? new TurnstileCaptcha(env.TURNSTILE_SECRET) : new DevCaptcha(),
    billing,
    config: new ConfigSigner(keys, undefined, env.MIN_APP_VERSION),
    events: new EventHub(redis, sub),
    auth: createAuth({ env, db, email, breach, deps: () => deps }),
    ai: overrides.ai ?? selectAi(env),
    stt: overrides.stt ?? selectStt(env),
    relay: new RelayControl(),
    ssoNet: overrides.ssoNet ?? { fetch: (input, init) => fetch(input, init), resolveTxt },
  };
  return {
    deps,
    close: async () => {
      await deps.events.close();
      redis.disconnect();
      sub.disconnect();
      await pool.end();
    },
  };
}
