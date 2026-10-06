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
- Telemetry (opt-in only, M2): a PostHog project key in `POSTHOG_KEY` and a Sentry DSN in `SENTRY_DSN`, set at desktop build time. Without them nothing is ever sent, even when a user opts in. Verify: with a debug build, `POSTHOG_KEY=… SENTRY_DSN=…` set, opt in under Settings › Privacy & Telemetry and see requests to the analytics host in `TANDEM_NET_LOG`.
- Production entitlement and config public keys bundled into `apps/desktop/src-tauri/resources/keys.json` for release builds (ARCHITECTURE §14.4); see "Production Ed25519 keys" above.

## Signing & distribution
- Not started. Signing secrets are listed in ARCHITECTURE §17.4 and are added in M5.

## Legal
- Trademark search for the working name "Tandem" before any public launch (DECISIONS §Identity).
- Engine marks: Tandem ships its own letter monograms (UI_SPEC §1.6). Before shipping any vendor logo, get each vendor's permission or trademark guidance.
- Review the UI labels the consistency pass flagged as generic but coincident with the reference product (CONSISTENCY_REPORT "Remaining known open items" 2).

## Manual QA checklist
- M2: the `tandem://` scheme on macOS and Windows (installed build): `tandem://open?path=/some/folder` opens the app and adds the folder.
- M2: macOS overlay title bar with the traffic lights inset; the left cluster starts clear of them.
- M2: Windows snap layouts appear when hovering the custom maximize button.
- M2: a cold start with Dark on a Light OS shows no white flash (F-SH-05 AC1; automated on Linux only via the native background).
- M3: real Japanese IME on macOS and Windows (commit on Enter, no CR during composition); Option-as-Meta on macOS.
- M3: ConPTY TUIs on Windows (vim, htop): rendering, resize, mouse; closing a pane ends its whole tree (Job Object) and a crash of Tandem leaves no children behind.
- M3: orphan detection on macOS after a forced quit (`kill -9`) with a `nohup` child in a pane: the dialog lists it and Stop ends it.
- M3: the 16-pane benchmark (`pnpm --filter desktop perf -- --suite terminals`) on the reference machines (a GPU, 8+ cores). The build VM (4 vCPU, Xvfb, software GL) records mode switch ~150 ms, six agents framed 670 ms / 526 ms stall, key echo P95 223 ms, focused 4.5 fps, idle CPU 6–13 %, RSS ~2.3 GB; compare and file anything still over target.
- M4: real engines per OS (Claude Code, Codex, Gemini, Copilot, Cursor, Grok, OpenCode, Droid, Amp, Aider, Kimi): detected path and version, launch in its provider home, sign-in stays inside that home; Claude hooks drive Working → Waiting → Done in the pane header and rail.
- M5: system notifications per OS: Windows toast with the AppUserModelID registered on the Start Menu shortcut (an installed build), macOS dock bounce and Linux urgency hint while in the background, and what clicking a notification does (D-78).
- M4: macOS GUI-PATH detection: an engine on PATH only through `~/.zprofile` (zsh) is found from the Dock-launched app, and one only in `~/.zshrc` gets the "Add the folder to PATH in ~/.zprofile" fix.
- M3: PowerShell shell integration on Windows (pwsh 7 and Windows PowerShell 5.1): cwd after `cd`, ✓/✗ marks, and "Copy last command output" without a leading blank line; the user's own `prompt` still renders.
- M3: zsh integration with a real oh-my-zsh / powerlevel10k setup (instant prompt on): prompt unchanged, marks present, `echo $ZDOTDIR` shows the user's own value.
