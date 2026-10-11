import { z } from "zod";

/** API configuration (ARCHITECTURE §17.5). Defaults are dev values. */
const schema = z.object({
  PORT: z.coerce.number().int().positive().default(8787),
  NODE_ENV: z.enum(["development", "test", "production"]).default("development"),
  LOG_LEVEL: z.string().default("info"),
  DATABASE_URL: z.string().default("postgres://tandem:tandem@localhost:5432/tandem"),
  REDIS_URL: z.string().default("redis://localhost:6379"),
  API_PUBLIC_URL: z.string().default("http://api.tandem.local:8787"),
  WEB_PUBLIC_URL: z.string().default("http://localhost:3000"),
  BETTER_AUTH_SECRET: z.string().min(16).default("tandem-dev-secret-change-me-0123456789"),
  JWT_SIGNING_KEYS: z.string().optional(),
  CONFIG_SIGNING_KEY: z.string().optional(),
  SMTP_URL: z.string().default("smtp://localhost:1025"),
  EMAIL_FROM: z.string().default("Tandem <no-reply@tandem.local>"),
  GOOGLE_CLIENT_ID: z.string().optional(),
  GOOGLE_CLIENT_SECRET: z.string().optional(),
  BILLING_PROVIDER: z.enum(["mock", "stripe"]).default("mock"),
  STRIPE_SECRET_KEY: z.string().default("sk_test_mock"),
  STRIPE_WEBHOOK_SECRET: z.string().default("whsec_mock"),
  STRIPE_API_BASE: z.string().optional(),
  STRIPE_PRICE_PRO_MONTH: z.string().default("price_mock_pro_month"),
  STRIPE_PRICE_PRO_YEAR: z.string().default("price_mock_pro_year"),
  STRIPE_PRICE_TEAM_MONTH: z.string().default("price_mock_team_month"),
  STRIPE_PRICE_TEAM_YEAR: z.string().default("price_mock_team_year"),
  STRIPE_PRICE_PACK_5K: z.string().default("price_mock_pack_5k"),
  STRIPE_PRICE_PACK_12K: z.string().default("price_mock_pack_12k"),
  STRIPE_PRICE_PACK_30K: z.string().default("price_mock_pack_30k"),
  TURNSTILE_SECRET: z.string().optional(),
  BREACH_CHECK: z.enum(["offline", "hibp"]).default("offline"),
  TANDEM_API_TEST_ROUTES: z.string().optional(),
  MIN_APP_VERSION: z.string().optional(),
  /** Hosted AI features (D-103): `claude` needs ANTHROPIC_API_KEY; `mock` answers deterministically. */
  AI_PROVIDER: z.enum(["mock", "claude"]).default("mock"),
  ANTHROPIC_API_KEY: z.string().optional(),
  ANTHROPIC_API_BASE: z.string().optional(),
  /** Cloud STT (F-BE-07): `openai_compat` needs the key, base and model; `mock` answers deterministically. */
  STT_PROVIDER: z.enum(["mock", "openai_compat"]).default("mock"),
  STT_API_KEY: z.string().optional(),
  STT_API_BASE: z.string().optional(),
  STT_MODEL: z.string().optional(),
  /** Conductor's realtime voice (F-BE-08): `openai` needs OPENAI_API_KEY; `mock` mints dev tokens. */
  REALTIME_PROVIDER: z.enum(["mock", "openai"]).default("mock"),
  OPENAI_API_KEY: z.string().optional(),
  OPENAI_API_BASE: z.string().optional(),
  REALTIME_MODEL: z.string().optional(),
  /** Phone push (F-RM-06): `live` sends through FCM and APNs where their credentials are set; `mock` records. */
  PUSH_PROVIDER: z.enum(["mock", "live"]).default("mock"),
  FCM_PROJECT_ID: z.string().optional(),
  FCM_CLIENT_EMAIL: z.string().optional(),
  FCM_PRIVATE_KEY: z.string().optional(),
  APNS_KEY_ID: z.string().optional(),
  APNS_TEAM_ID: z.string().optional(),
  APNS_PRIVATE_KEY: z.string().optional(),
  APNS_TOPIC: z.string().default("com.vibezeo.remote"),
  /** Dev and E2E: where MockRealtime points calls (`scripts/mock-realtime.ts`, D-1102). */
  REALTIME_MOCK_URL: z.string().optional(),
  /** Team SSO over SAML through WorkOS (F-BL-06 P2, D-625); unset means OIDC only. */
  WORKOS_CLIENT_ID: z.string().optional(),
  WORKOS_API_KEY: z.string().optional(),
  WORKOS_API_BASE: z.string().optional(),
});

export type Env = z.infer<typeof schema>;

export function loadEnv(source: Record<string, string | undefined> = process.env): Env {
  return schema.parse(source);
}

/** `/__test/*` routes exist only with the flag and never in production. */
export function testRoutesEnabled(env: Env): boolean {
  return env.TANDEM_API_TEST_ROUTES === "1" && env.NODE_ENV !== "production";
}
