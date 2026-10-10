import type { BetterAuthPlugin } from "better-auth";
import { createAuthEndpoint } from "better-auth/api";
import { setSessionCookie } from "better-auth/cookies";
import { z } from "zod";
import type { Deps } from "../deps";
import { admit, finishSso, recordSignIn } from "../services/sso";

export const SSO_CALLBACK_PATH = "/sso/callback";

/**
 * Team SSO's return leg (F-BL-06 P2): `GET /v1/auth/sso/callback` checks the
 * identity provider's answer, finds or creates the account, joins the org and
 * signs in with an ordinary session cookie. Failures go back to the sign-in
 * page with a code it explains.
 */
export function ssoPlugin(getDeps: () => Deps, webUrl: string): BetterAuthPlugin {
  return {
    id: "vibezeo-sso",
    endpoints: {
      ssoCallback: createAuthEndpoint(
        SSO_CALLBACK_PATH,
        {
          method: "GET",
          query: z.object({
            state: z.string().max(200).optional(),
            code: z.string().max(4000).optional(),
            error: z.string().max(200).optional(),
          }),
        },
        async (ctx) => {
          const deps = getDeps();
          const fail = (code: string) => ctx.redirect(`${webUrl}/signin?sso_error=${encodeURIComponent(code)}`);
          const out = await finishSso(deps, ctx.query);
          if (!out.ok) throw fail(out.code);
          const adapter = ctx.context.internalAdapter;
          const found = await adapter.findUserByEmail(out.profile.email);
          let user = found?.user ?? null;
          if (!user) {
            user = await adapter.createUser({
              email: out.profile.email,
              name: out.profile.name ?? out.profile.email.split("@")[0] ?? "",
              emailVerified: true,
            }, {
              method: out.provider === "workos" ? "sso-saml" : "sso-oidc",
              sso: { providerId: `vibezeo-sso:${out.orgId}` },
            });
          } else if (!user.emailVerified) {
            // The org's provider vouches for an address on its verified domain.
            user = await adapter.updateUser(user.id, { emailVerified: true });
          }
          if (!user) throw fail("SSO_ACCOUNT");
          const joined = await admit(deps, out.orgId, user.id);
          if (!joined.ok) throw fail(joined.code);
          const session = await adapter.createSession(user.id);
          if (!session) throw fail("SSO_ACCOUNT");
          await setSessionCookie(ctx, { session, user });
          await recordSignIn(deps, out.orgId, user.id);
          throw ctx.redirect(`${webUrl}${out.callback}`);
        },
      ),
    },
  };
}
