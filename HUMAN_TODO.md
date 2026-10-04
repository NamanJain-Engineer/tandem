# HUMAN_TODO

Things only a human can do. The build never blocks on them: each sits behind a mock or dev provider.

## Accounts & keys
- Enable GitHub Actions on `NamanJain-Engineer/tandem` so `.github/workflows/ci.yml` runs.
- Google OAuth client: set `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` for the API; redirect URI `https://app.tandem.dev/v1/auth/callback/google`. Until then the Google button is hidden. Verify: the sign-in page offers Google and the callback lands on /account.
- Stripe: create products and prices for Pro $29/mo and $290/yr (7-day trial, card required) and Team $49/seat/mo and $490/seat/yr (14-day trial); credit packs 5k/$5, 12k/$10, 30k/$20. Put the price ids in `STRIPE_PRICE_*`, the key in `STRIPE_SECRET_KEY`, set `BILLING_PROVIDER=stripe`. Configure the customer portal; set dunning to cancel after 14 days; add the webhook endpoint `https://api.tandem.dev/v1/webhooks/stripe` and put its secret in `STRIPE_WEBHOOK_SECRET`. Verify: `bash scripts/stripe-fixture.sh invoice.paid <email>` with the real secret returns 200.
- SMTP provider credentials in `SMTP_URL` and a verified sender for `EMAIL_FROM`.
- Cloudflare Turnstile site and secret keys (`TURNSTILE_SECRET`; site key for the web sign-up page). Edge rules must skip challenges for `Authorization: Bearer` requests and `/v1/webhooks/*`.
- Production Ed25519 keys: generate with `pnpm keys:dev --force` on a secure machine (or a KMS), set `JWT_SIGNING_KEYS` (ak-*, ek-*) and `CONFIG_SIGNING_KEY` (ck-*), and copy the public keys into `apps/desktop/src-tauri/resources/keys.json` for release builds. Rotation: add the new kid to keys.json one release before signing with it.
- Managed Postgres 16 and Redis 7 (`DATABASE_URL`, `REDIS_URL`), `BETTER_AUTH_SECRET` (32+ random bytes), `BREACH_CHECK=hibp`.
- Domains `api.tandem.dev` and `app.tandem.dev` (`API_PUBLIC_URL`, `WEB_PUBLIC_URL`).

## Signing & distribution
- Not started. Signing secrets are listed in ARCHITECTURE §17.4 and are added in M5.

## Legal
- Trademark search for the working name "Tandem" before any public launch (DECISIONS §Identity).
- Review the UI labels the consistency pass flagged as generic but coincident with the reference product (CONSISTENCY_REPORT "Remaining known open items" 2).

## Manual QA checklist
- None yet. M0 is covered by automated tests, including the desktop boot E2E under xvfb.
