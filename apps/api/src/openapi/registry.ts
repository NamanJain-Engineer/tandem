// Public API routes and their zod schemas, the source for openapi.json (F-WB-04).
import { z } from "zod";
import { rewriteBody, titleBody } from "../routes/ai";
import { checkoutBody, packBody } from "../routes/billing";
import { authorizeBody, exchangeBody, refreshBody, tokenResponse } from "../routes/desktopAuth";

const problem = z.object({
  type: z.string(),
  title: z.string(),
  status: z.number().int(),
  code: z.string().optional(),
  detail: z.string().optional(),
});
const health = z.object({ ok: z.literal(true), version: z.string() });
const signedConfig = z.object({ payload: z.string(), sig: z.string(), kid: z.string() });
const url = z.object({ url: z.string() });
const credits = z.object({
  balance: z.object({ monthly: z.number(), purchased: z.number(), resetsAt: z.string().nullable() }),
  ledger: z.array(
    z.object({
      id: z.string(),
      bucket: z.string(),
      delta: z.number(),
      reason: z.string(),
      createdAt: z.string(),
    }),
  ),
});
const me = z.object({
  user: z.object({
    id: z.string(),
    email: z.string(),
    name: z.string(),
    emailVerified: z.boolean(),
    role: z.string(),
  }),
  plan: z.enum(["free", "pro", "team"]),
  status: z.string(),
  limits: z.record(z.string(), z.unknown()),
  paid_until: z.string().nullable(),
  cancel_at_period_end: z.boolean(),
  credits: z.object({ monthly: z.number(), purchased: z.number(), resets_at: z.string().nullable() }),
  flags: z.record(z.string(), z.boolean()),
  policy: z.record(z.string(), z.unknown()).nullable(),
  org: z.null(),
  entitlement_token: z.string(),
});
const devices = z.array(
  z.object({
    id: z.string(),
    deviceName: z.string().nullable(),
    platform: z.string().nullable(),
    appVersion: z.string().nullable(),
    lastUsedAt: z.string(),
  }),
);

export type RouteDoc = {
  method: "get" | "post" | "delete";
  path: string;
  summary: string;
  auth: "none" | "bearer" | "cookie" | "bearer_or_cookie" | "stripe_signature";
  body?: z.ZodType;
  response?: z.ZodType;
  status?: number;
};

export const ROUTES: RouteDoc[] = [
  { method: "get", path: "/v1/health", summary: "Liveness", auth: "none", response: health },
  {
    method: "get",
    path: "/v1/config",
    summary: "Signed remote config (ETag, 10 min cache)",
    auth: "none",
    response: signedConfig,
  },
  {
    method: "post",
    path: "/v1/auth/desktop/authorize",
    summary: "One-time desktop sign-in code",
    auth: "cookie",
    body: authorizeBody,
    response: z.object({ code: z.string() }),
  },
  {
    method: "post",
    path: "/v1/auth/desktop/exchange",
    summary: "Exchange a code and PKCE verifier for tokens",
    auth: "none",
    body: exchangeBody,
    response: tokenResponse,
  },
  {
    method: "post",
    path: "/v1/auth/desktop/refresh",
    summary: "Rotate a refresh token",
    auth: "none",
    body: refreshBody,
    response: tokenResponse,
  },
  {
    method: "post",
    path: "/v1/auth/desktop/logout",
    summary: "End a desktop session",
    auth: "bearer",
    body: refreshBody,
    status: 204,
  },
  {
    method: "get",
    path: "/v1/me",
    summary: "User, plan, limits, credits and entitlement token",
    auth: "bearer_or_cookie",
    response: me,
  },
  {
    method: "get",
    path: "/v1/me/events",
    summary: "SSE: entitlement and credits invalidations",
    auth: "bearer_or_cookie",
  },
  {
    method: "get",
    path: "/v1/me/sessions",
    summary: "Desktop sessions (devices)",
    auth: "cookie",
    response: devices,
  },
  {
    method: "delete",
    path: "/v1/me/sessions/{id}",
    summary: "Revoke a desktop session",
    auth: "cookie",
    status: 204,
  },
  {
    method: "delete",
    path: "/v1/me",
    summary: "Request account deletion",
    auth: "cookie",
    body: z.object({ confirm: z.literal("DELETE") }),
    status: 202,
  },
  {
    method: "get",
    path: "/v1/credits",
    summary: "Credit balance and recent ledger",
    auth: "bearer_or_cookie",
    response: credits,
  },
  {
    method: "post",
    path: "/v1/billing/checkout",
    summary: "Start a subscription checkout",
    auth: "bearer_or_cookie",
    body: checkoutBody,
    response: url,
  },
  {
    method: "post",
    path: "/v1/billing/portal",
    summary: "Open the billing portal",
    auth: "bearer_or_cookie",
    response: url,
  },
  {
    method: "post",
    path: "/v1/billing/credit-packs/checkout",
    summary: "Buy a credit pack (Pro/Team)",
    auth: "bearer_or_cookie",
    body: packBody,
    response: url,
  },
  {
    method: "post",
    path: "/v1/ai/title",
    summary: "Title a session from its opening text (Claude API, free, 60/h)",
    auth: "bearer",
    body: titleBody,
    response: z.object({ title: z.string().max(60) }),
  },
  {
    method: "post",
    path: "/v1/ai/rewrite",
    summary: "Enhance or Polish dictated text (Claude API, Pro, 3 credits)",
    auth: "bearer",
    body: rewriteBody,
    response: z.object({ text: z.string() }),
  },
  {
    method: "post",
    path: "/v1/webhooks/stripe",
    summary: "Stripe webhooks (signature verified)",
    auth: "stripe_signature",
    response: z.object({ received: z.literal(true) }),
  },
];

export function buildOpenApi(version: string): Record<string, unknown> {
  const paths: Record<string, Record<string, unknown>> = {};
  const security: Record<RouteDoc["auth"], unknown[]> = {
    none: [],
    bearer: [{ bearer: [] }],
    cookie: [{ cookie: [] }],
    bearer_or_cookie: [{ bearer: [] }, { cookie: [] }],
    stripe_signature: [{ stripeSignature: [] }],
  };
  for (const r of ROUTES) {
    const status = String(r.status ?? 200);
    const op: Record<string, unknown> = {
      summary: r.summary,
      security: security[r.auth],
      responses: {
        [status]: r.response
          ? {
              description: "OK",
              content: { "application/json": { schema: z.toJSONSchema(r.response, { io: "output" }) } },
            }
          : { description: status === "200" ? "OK" : "No content" },
        default: {
          description: "Problem",
          content: { "application/problem+json": { schema: z.toJSONSchema(problem) } },
        },
      },
    };
    if (r.body) {
      op.requestBody = {
        required: true,
        content: { "application/json": { schema: z.toJSONSchema(r.body, { io: "input" }) } },
      };
    }
    paths[r.path] = { ...(paths[r.path] ?? {}), [r.method]: op };
  }
  return {
    openapi: "3.1.0",
    info: { title: "Tandem API", version },
    servers: [{ url: "https://api.tandem.dev" }],
    components: {
      securitySchemes: {
        bearer: { type: "http", scheme: "bearer", bearerFormat: "JWT" },
        cookie: { type: "apiKey", in: "cookie", name: "tandem.session_token" },
        stripeSignature: { type: "apiKey", in: "header", name: "stripe-signature" },
      },
    },
    paths,
  };
}
