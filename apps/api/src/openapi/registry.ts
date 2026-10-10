// Public API routes and their zod schemas, the source for openapi.json (F-WB-04).
import { z } from "zod";
import { grantBody, refundBody } from "../routes/admin";
import { draftBody, rewriteBody, titleBody } from "../routes/ai";
import { autoNextBody } from "../routes/auto";
import { createPairingBody } from "../services/pairings";
import { acceptBody, createOrgBody, inviteBody, memberBody, seatsBody } from "../routes/orgs";
import { clientAuditBody, policySchema } from "../services/orgs";
import { domainBody, ssoBody } from "../services/sso";
import { ssoLookupBody, ssoStartBody } from "../routes/sso";
import { checkoutBody, packBody } from "../routes/billing";
import { authorizeBody, exchangeBody, refreshBody, tokenResponse } from "../routes/desktopAuth";
import { approveBody, startBody, startResponse, tokenBody } from "../routes/deviceAuth";

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
  org: z.object({ id: z.string(), cap: z.number().nullable(), spent: z.number() }).nullable(),
  ledger: z.array(
    z.object({
      id: z.string(),
      bucket: z.string(),
      delta: z.number(),
      reason: z.string(),
      feature: z.string().nullable(),
      createdAt: z.string(),
    }),
  ),
});
const orgView = z.object({
  id: z.string(),
  name: z.string(),
  role: z.enum(["owner", "admin", "member"]),
  plan: z.enum(["free", "pro", "team"]),
  status: z.string(),
  seats: z.number().nullable(),
  memberCount: z.number(),
  members: z.array(
    z.object({
      userId: z.string(),
      email: z.string(),
      name: z.string(),
      role: z.enum(["owner", "admin", "member"]),
      monthlyCap: z.number().nullable(),
      spent: z.number(),
      joinedAt: z.string(),
    }),
  ),
  invites: z.array(z.object({ id: z.string(), email: z.string(), role: z.string(), expiresAt: z.string() })),
  policy: policySchema,
  credits: z.object({ monthly: z.number(), purchased: z.number(), resetsAt: z.string().nullable() }),
});
const sttForm = z.object({
  audio: z
    .string()
    .describe("Ogg Opus or WAV, at most 10 minutes and 25 MB. Send it after the other fields."),
  duration_ms: z
    .string()
    .optional()
    .describe("The take's length; the relay reserves for it and holds the audio to it."),
  language: z.string().optional().describe("A language code, or auto (the default)."),
  hints: z.array(z.string()).max(50).optional().describe("Preferred words, only with the user's opt-in."),
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
  credits: z.object({
    monthly: z.number(),
    purchased: z.number(),
    resets_at: z.string().nullable(),
    cap: z.number().nullable(),
    spent: z.number().nullable(),
    grant: z.number(),
  }),
  flags: z.record(z.string(), z.boolean()),
  policy: policySchema.nullable(),
  org: z.object({ id: z.string(), name: z.string(), role: z.enum(["owner", "admin", "member"]) }).nullable(),
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

const SSO_VIEW = z.object({
      connection: z
        .object({
          provider: z.enum(["oidc", "workos", "mock"]),
          issuer: z.string().nullable(),
          clientId: z.string().nullable(),
          hasSecret: z.boolean(),
          organization: z.string().nullable(),
          enforce: z.boolean(),
          updatedAt: z.string(),
        })
        .nullable(),
      domains: z.array(
        z.object({
          domain: z.string(),
          verified: z.boolean(),
          record: z.object({ type: z.literal("TXT"), name: z.string(), value: z.string() }),
        }),
      ),
      providers: z.object({ oidc: z.boolean(), workos: z.boolean() }),
      redirectUri: z.string(),
    });

export type RouteDoc = {
  method: "get" | "post" | "delete" | "put" | "patch";
  path: string;
  summary: string;
  auth: "none" | "bearer" | "cookie" | "bearer_or_cookie" | "stripe_signature" | "admin";
  body?: z.ZodType;
  /** A multipart/form-data body instead of JSON. */
  multipart?: z.ZodType;
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
    method: "post",
    path: "/v1/auth/device/start",
    summary: "Start a device-code sign-in (CLI, headless runs; 10 an hour per IP)",
    auth: "none",
    body: startBody,
    response: startResponse,
  },
  {
    method: "get",
    path: "/v1/auth/device/info",
    summary: "Which app a user code belongs to (the approval page)",
    auth: "cookie",
    response: z.object({ user_code: z.string(), client: z.string(), version: z.string(), expires_at: z.string() }),
  },
  {
    method: "post",
    path: "/v1/auth/device/approve",
    summary: "Approve or deny a user code",
    auth: "cookie",
    body: approveBody,
    status: 204,
  },
  {
    method: "post",
    path: "/v1/auth/device/token",
    summary: "Poll for tokens (400 authorization_pending, slow_down, expired_token, access_denied)",
    auth: "none",
    body: tokenBody,
    response: tokenResponse,
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
    path: "/v1/ai/teammate-draft",
    summary: "Draft a teammate from a job description (Claude API, Pro, 10 credits)",
    auth: "bearer",
    body: draftBody,
    response: z.object({
      name: z.string(),
      brief: z.string(),
      engine: z.string(),
      plugins: z.array(z.string()),
    }),
  },
  {
    method: "post",
    path: "/v1/pairings",
    summary: "Register a phone pairing: a token for the phone and the relay URL (Pro)",
    auth: "bearer",
    body: createPairingBody,
    response: z.object({ pairing_token: z.string(), relay_url: z.string(), expires_at: z.string() }),
  },
  {
    method: "delete",
    path: "/v1/pairings/{id}",
    summary: "Revoke a phone pairing; the relay drops both sockets",
    auth: "bearer",
    status: 204,
  },
  {
    method: "get",
    path: "/v1/relay/{pairing_id}",
    summary:
      "Phone relay WebSocket: the desktop (bearer) or the phone (Sec-WebSocket-Protocol tandem-pair.<token>); opaque binary frames up to 64 KB, 1009 when larger, 1008 when refused",
    auth: "bearer",
    status: 101,
  },
  {
    method: "post",
    path: "/v1/auto/next",
    summary: "One Auto turn: the supervisor's next prompt, or done or stuck (Claude API, Pro, 6 credits)",
    auth: "bearer",
    body: autoNextBody,
    response: z.object({
      action: z.enum(["prompt", "done", "stuck"]),
      text: z.string().optional(),
      reason: z.string().optional(),
    }),
  },
  {
    method: "post",
    path: "/v1/stt/transcribe",
    summary: "Cloud speech-to-text (Pro, 2 credits per started 15 s, 120 min a day)",
    auth: "bearer",
    multipart: sttForm,
    response: z.object({ text: z.string(), language: z.string().nullable(), duration: z.number() }),
  },
  {
    method: "post",
    path: "/v1/audit/events",
    summary: "Metadata-only audit events from a Team member's app (kept when the org's audit is on)",
    auth: "bearer",
    body: clientAuditBody,
    status: 204,
  },
  {
    method: "post",
    path: "/v1/orgs",
    summary: "Create an organization",
    auth: "cookie",
    body: createOrgBody,
    response: orgView,
    status: 201,
  },
  {
    method: "post",
    path: "/v1/orgs/invites/accept",
    summary: "Accept an emailed invite",
    auth: "cookie",
    body: acceptBody,
    response: z.object({ orgId: z.string() }),
  },
  {
    method: "get",
    path: "/v1/orgs/{id}",
    summary: "An organization (members see their own row)",
    auth: "bearer_or_cookie",
    response: orgView,
  },
  {
    method: "post",
    path: "/v1/orgs/{id}/invites",
    summary: "Invite by email (owner or admin)",
    auth: "cookie",
    body: inviteBody,
    response: z.object({ id: z.string() }),
    status: 201,
  },
  {
    method: "patch",
    path: "/v1/orgs/{id}/members/{userId}",
    summary: "Change a member's role or monthly credit cap",
    auth: "cookie",
    body: memberBody,
    status: 204,
  },
  {
    method: "delete",
    path: "/v1/orgs/{id}/members/{userId}",
    summary: "Remove a member, or leave",
    auth: "cookie",
    status: 204,
  },
  {
    method: "put",
    path: "/v1/orgs/{id}/policy",
    summary: "Set the admin policy",
    auth: "cookie",
    body: policySchema,
    response: policySchema,
  },
  {
    method: "put",
    path: "/v1/orgs/{id}/seats",
    summary: "Set the seat count (the subscription's quantity follows)",
    auth: "cookie",
    body: seatsBody,
    response: z.object({ seats: z.number() }),
  },
  {
    method: "get",
    path: "/v1/orgs/{id}/sso",
    summary: "The org's single sign-on connection and domains (owner or admin)",
    auth: "cookie",
    response: SSO_VIEW,
  },
  {
    method: "put",
    path: "/v1/orgs/{id}/sso",
    summary: "Set the identity provider (OIDC, or SAML through WorkOS) and whether SSO is required",
    auth: "cookie",
    body: ssoBody,
    response: SSO_VIEW,
  },
  {
    method: "delete",
    path: "/v1/orgs/{id}/sso",
    summary: "Remove single sign-on",
    auth: "cookie",
    status: 204,
  },
  {
    method: "post",
    path: "/v1/orgs/{id}/sso/domains",
    summary: "Add an email domain; it counts once its DNS TXT record is verified",
    auth: "cookie",
    body: domainBody,
    response: SSO_VIEW,
    status: 201,
  },
  {
    method: "post",
    path: "/v1/orgs/{id}/sso/domains/{domain}/verify",
    summary: "Check the domain's TXT record",
    auth: "cookie",
    response: SSO_VIEW,
  },
  {
    method: "delete",
    path: "/v1/orgs/{id}/sso/domains/{domain}",
    summary: "Remove a domain",
    auth: "cookie",
    status: 204,
  },
  {
    method: "post",
    path: "/v1/sso/lookup",
    summary: "Whether an address signs in with its organization's SSO, and must",
    auth: "none",
    body: ssoLookupBody,
    response: z.object({ sso: z.boolean(), required: z.boolean(), org: z.string().nullable().optional() }),
  },
  {
    method: "post",
    path: "/v1/sso/start",
    summary: "Where to send the browser to sign in with the organization's identity provider",
    auth: "none",
    body: ssoStartBody,
    response: z.object({ url: z.string() }),
  },
  {
    method: "get",
    path: "/v1/orgs/{id}/audit",
    summary: "The org audit log (owner or admin), newest first",
    auth: "cookie",
    response: z.object({
      events: z.array(
        z.object({
          id: z.string(),
          event: z.string(),
          meta: z.unknown(),
          createdAt: z.string(),
          actorEmail: z.string().nullable(),
        }),
      ),
      next: z.string().nullable(),
    }),
  },
  {
    method: "get",
    path: "/v1/admin/users",
    summary: "Look users up by email (?q=, at least 2 characters, 20 results)",
    auth: "admin",
    response: z.object({
      users: z.array(z.object({ id: z.string(), email: z.string(), name: z.string(), role: z.string(), createdAt: z.string() })),
    }),
  },
  {
    method: "get",
    path: "/v1/admin/users/{id}",
    summary: "A user's plan, subscription, org, credits, last 50 ledger rows and the admin actions on them",
    auth: "admin",
    response: z.object({ user: z.object({ id: z.string(), email: z.string() }).passthrough(), plan: z.string() }).passthrough(),
  },
  {
    method: "post",
    path: "/v1/admin/users/{id}/credits",
    summary: "Grant credits (to whoever pays for the user) with a reason; recorded as an admin action",
    auth: "admin",
    body: grantBody,
    response: z.object({ ledgerEntry: z.string(), balance: z.number() }),
  },
  {
    method: "post",
    path: "/v1/admin/users/{id}/refund",
    summary: "Refund a first plan charge within 14 days, or an unused credit pack",
    auth: "admin",
    body: refundBody,
    response: z.object({ refunded: z.enum(["plan", "pack"]) }).passthrough(),
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
    admin: [{ cookie: [] }],
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
    if (r.multipart) {
      op.requestBody = {
        required: true,
        content: { "multipart/form-data": { schema: z.toJSONSchema(r.multipart, { io: "input" }) } },
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
