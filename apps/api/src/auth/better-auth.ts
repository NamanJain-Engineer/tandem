import { eq } from "drizzle-orm";
import { betterAuth } from "better-auth";
import { drizzleAdapter } from "better-auth/adapters/drizzle";
import { APIError, createAuthMiddleware } from "better-auth/api";
import { magicLink } from "better-auth/plugins";
import type { Db } from "../db/client";
import type { Deps } from "../deps";
import * as schema from "../db/schema";
import type { Env } from "../env";
import type { BreachChecker } from "../providers/breach";
import type { EmailProvider } from "../providers/email/types";
import { ssoRequiredFor } from "../services/sso";
import { SSO_CALLBACK_PATH, ssoPlugin } from "./ssoPlugin";

export const BREACHED_MESSAGE = "This password has appeared in a data breach. Choose a different one.";
const PASSWORD_PATHS = new Set(["/sign-up/email", "/reset-password", "/change-password"]);

export type AuthDeps = {
  env: Env;
  db: Db;
  email: EmailProvider;
  breach: BreachChecker;
  /** The whole app's deps, read at request time (Team SSO). */
  deps: () => Deps;
};

/** A deleted account stays closed while its data is removed (F-TL-05). */
export const ACCOUNT_DELETED_MESSAGE = "This account is being deleted, so it can't be signed in to.";

export const SSO_REQUIRED_MESSAGE = "Your organization signs in with single sign-on. Use “Sign in with SSO”.";
/** Ways in that skip the org's identity provider (F-BL-06 P2 "SSO only"). */
const SSO_GUARDED_PATHS = new Set(["/sign-in/email", "/sign-in/magic-link", "/sign-up/email"]);

/**
 * better-auth (ARCHITECTURE §14.3): email + password with verification, magic
 * link, Google when configured, 30-minute reset links. Requests reach it
 * through the web app's `/v1/*` rewrite, so its base URL is the web origin and
 * cookies are first-party there.
 */
export function createAuth({ env, db, email, breach, deps }: AuthDeps) {
  const google =
    env.GOOGLE_CLIENT_ID && env.GOOGLE_CLIENT_SECRET
      ? { google: { clientId: env.GOOGLE_CLIENT_ID, clientSecret: env.GOOGLE_CLIENT_SECRET } }
      : {};
  return betterAuth({
    appName: "Tandem",
    baseURL: env.WEB_PUBLIC_URL,
    basePath: "/v1/auth",
    secret: env.BETTER_AUTH_SECRET,
    database: drizzleAdapter(db, {
      provider: "pg",
      schema: {
        user: schema.user,
        session: schema.session,
        account: schema.account,
        verification: schema.verification,
      },
    }),
    trustedOrigins: [
      env.WEB_PUBLIC_URL,
      env.API_PUBLIC_URL,
      "http://localhost:3000",
      "http://127.0.0.1:3000",
    ],
    telemetry: { enabled: false },
    user: {
      additionalFields: {
        role: { type: "string", required: false, defaultValue: "user", input: false },
        deletionRequestedAt: { type: "date", required: false, input: false },
      },
    },
    emailAndPassword: {
      enabled: true,
      requireEmailVerification: true,
      minPasswordLength: 10,
      maxPasswordLength: 256,
      resetPasswordTokenExpiresIn: 1800,
      sendResetPassword: async ({ user, url }) => {
        await email.send({
          to: user.email,
          subject: "Reset your Tandem password",
          text: `Reset your password with this link. It expires in 30 minutes.\n\n${url}\n\nIf you didn't ask for this, ignore this email.`,
        });
      },
    },
    emailVerification: {
      sendOnSignUp: true,
      autoSignInAfterVerification: true,
      sendVerificationEmail: async ({ user, url }) => {
        await email.send({
          to: user.email,
          subject: "Verify your email for Tandem",
          text: `Confirm your email address to finish creating your Tandem account:\n\n${url}`,
        });
      },
    },
    socialProviders: { ...google },
    plugins: [
      ssoPlugin(deps, env.WEB_PUBLIC_URL),
      magicLink({
        expiresIn: 600,
        sendMagicLink: async ({ email: to, url }) => {
          await email.send({
            to,
            subject: "Your Tandem sign-in link",
            text: `Sign in to Tandem:\n\n${url}\n\nThe link expires in 10 minutes.`,
          });
        },
      }),
    ],
    advanced: {
      cookiePrefix: "tandem",
      useSecureCookies: env.WEB_PUBLIC_URL.startsWith("https://"),
      database: { generateId: () => crypto.randomUUID() },
    },
    databaseHooks: {
      session: {
        create: {
          // The backstop for deleted accounts and for "SSO only" (no session
          // for an enforced address except from the SSO callback), whatever
          // way in was tried.
          before: async (session, ctx) => {
            const [u] = await db
              .select({ email: schema.user.email, deletionRequestedAt: schema.user.deletionRequestedAt })
              .from(schema.user)
              .where(eq(schema.user.id, session.userId))
              .limit(1);
            // No way back in once "Delete account" ran, SSO included (F-TL-05).
            if (u?.deletionRequestedAt) {
              throw new APIError("FORBIDDEN", { message: ACCOUNT_DELETED_MESSAGE, code: "ACCOUNT_DELETED" });
            }
            if (ctx?.path === SSO_CALLBACK_PATH) return;
            if (u && (await ssoRequiredFor(deps(), u.email))) {
              throw new APIError("FORBIDDEN", { message: SSO_REQUIRED_MESSAGE, code: "SSO_REQUIRED" });
            }
          },
        },
      },
    },
    hooks: {
      before: createAuthMiddleware(async (ctx) => {
        if (SSO_GUARDED_PATHS.has(ctx.path)) {
          const address = (ctx.body as { email?: unknown } | undefined)?.email;
          if (typeof address === "string" && (await ssoRequiredFor(deps(), address))) {
            throw new APIError("FORBIDDEN", { message: SSO_REQUIRED_MESSAGE, code: "SSO_REQUIRED" });
          }
        }
        if (!PASSWORD_PATHS.has(ctx.path)) return;
        const body = (ctx.body ?? {}) as { password?: unknown; newPassword?: unknown };
        const candidate = typeof body.newPassword === "string" ? body.newPassword : body.password;
        if (typeof candidate === "string" && (await breach.isBreached(candidate))) {
          throw new APIError("BAD_REQUEST", { message: BREACHED_MESSAGE, code: "PASSWORD_BREACHED" });
        }
      }),
    },
  });
}

export type Auth = ReturnType<typeof createAuth>;

export const SESSION_COOKIE = "tandem.session_token";
