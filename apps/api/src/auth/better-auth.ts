import { betterAuth } from "better-auth";
import { drizzleAdapter } from "better-auth/adapters/drizzle";
import { APIError, createAuthMiddleware } from "better-auth/api";
import { magicLink } from "better-auth/plugins";
import type { Db } from "../db/client";
import * as schema from "../db/schema";
import type { Env } from "../env";
import type { BreachChecker } from "../providers/breach";
import type { EmailProvider } from "../providers/email/types";

export const BREACHED_MESSAGE = "This password has appeared in a data breach. Choose a different one.";
const PASSWORD_PATHS = new Set(["/sign-up/email", "/reset-password", "/change-password"]);

export type AuthDeps = { env: Env; db: Db; email: EmailProvider; breach: BreachChecker };

/**
 * better-auth (ARCHITECTURE §14.3): email + password with verification, magic
 * link, Google when configured, 30-minute reset links. Requests reach it
 * through the web app's `/v1/*` rewrite, so its base URL is the web origin and
 * cookies are first-party there.
 */
export function createAuth({ env, db, email, breach }: AuthDeps) {
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
    hooks: {
      before: createAuthMiddleware(async (ctx) => {
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
