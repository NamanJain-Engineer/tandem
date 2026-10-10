# Operating Vibezeo: the handbook

How to build, test, release, deploy and check Vibezeo end to end. It's
written for a coding agent picking the project up cold (and works for a
person too). Read it after `CLAUDE.md`; every rule there still applies.

The product is **Vibezeo**, a desktop workspace for coding-agent CLIs. The code
still uses the working name *tandem* (crates, commands, bundle id); keep it.

## 1. Where everything is

| What | Where |
|---|---|
| Rules for agents | `CLAUDE.md` (Always / Never / Conventions) |
| Where the build is now, every decision (D-n), the log of each slice | `BUILD_LOG.md`: read "Current state" first |
| What to build, in order, with acceptance tests | `build-kit/MILESTONES.md` (M0–M15) |
| Every feature still to build, in order | `docs/REMAINING.md` |
| Each remaining feature's behaviour and acceptance tests, in build order | `docs/FEATURES_TO_BUILD.md` |
| Feature behaviour and acceptance criteria (F-AREA-nn) | `build-kit/PRODUCT_SPEC.md` |
| Crates, IPC commands and events, data model, protocols | `build-kit/ARCHITECTURE.md` |
| Fixed stack, names, layout (never re-decide) | `build-kit/DECISIONS.md` |
| Screens and design tokens | `build-kit/UI_SPEC.md` |
| Things only a person can do (accounts, keys, manual QA) | `HUMAN_TODO.md` |
| Release notes | `CHANGELOG.md` |
| The public site's hosting | `docs/DEPLOY_SITE.md` |
| Helper scripts for agents | `scripts/agent/` |
| Product video sources | `media/product-video/` |

Code layout: `apps/desktop` (Tauri 2: Rust in `src-tauri`, React 19 + zustand
in `src`), `apps/web` (the site and account portal, Next.js; the public site is
its static export), `apps/api` (Hono backend, not deployed yet),
`crates/tandem-*` (core, store, engines, gateway, git, pty, …),
`packages/ui` (the only components and tokens UI code may use),
`packages/protocol` (TypeScript types generated from Rust by ts-rs, never
hand-edited).

## 2. Branches and what deploys

- **`main` deploys vibezeo.com** (Vercel, Root Directory `apps/web`). Never push
  to it without the owner's explicit go-ahead for that deploy.
- Product work happens on a product branch (currently `product-m0-m5`, which
  holds everything up to 0.2.0). Each agent session works on its own branch and
  merges or opens a pull request into the product branch.
- Parallel sessions: give each its own milestone, number its new decisions in its
  own range (for example D-200+), and add BUILD_LOG entries only under its own
  milestone. Merge the product branch in often; shared files (`BUILD_LOG.md`,
  `apps/desktop/src/i18n/en.json`, the command registry, `ARCHITECTURE.md`) get
  small conflicts.
- Milestone tags (`m0`…`m7`) have been local only; the BUILD_LOG records the same
  points. Push tags if the remote accepts them.

## 3. Setting up a machine (Ubuntu 24.04 / Debian 12, root)

```bash
bash scripts/bootstrap-linux.sh                  # WebKitGTK, webkit2gtk-driver, xvfb, dbus, openbox, xclip, tauri-driver,
                                                 # openssh-client (ssh, ssh-keygen, ssh-keyscan) and musl-tools, …
apt-get install -y zsh fish                      # the shell-integration E2E specs need them
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl  # tandem-host (M14)
pip install ziglang==0.13.0 && cargo install cargo-zigbuild --locked     # release-local.sh: the aarch64 helper
cargo install cargo-audit --locked
cargo install cargo-about --locked --features cli # release-local.sh: notices
cargo install cargo-cyclonedx --locked           # release-local.sh: SBOMs
pnpm install
pnpm keys:dev                                    # local Ed25519 dev keys in .dev-keys/ (git-ignored)
sh scripts/agent/with-infra.sh true              # proves Docker + Postgres/Redis/Mailpit/stripe-mock come up
```

- Docker must be started by hand in a fresh container; `scripts/agent/with-infra.sh CMD`
  starts `dockerd`, runs `pnpm infra:up`, runs CMD, and stops it again.
- Chromium for scripted browsers is at `/opt/pw-browsers/chromium` in Claude's cloud
  containers (never run `playwright install` there).
- **Disk is the usual constraint** (`target/debug` grows past 15 GB). Before big
  builds: `python3 scripts/agent/prune-target.py` (keep 3 builds per crate; pass `1`
  after a version bump), `rm -rf target/debug/examples target/perf`. Set
  `CARGO_INCREMENTAL=0`.
- The shell may be zsh: an unquoted `$VAR` holding several paths is **one** word
  there. Use arrays or quote per item.
- Docker Hub may answer image pulls with `429 Too Many Requests`, and
  `with-infra.sh` hides the failure (the run then fails later: sign-up 500s,
  `ECONNREFUSED 127.0.0.1:5432`). Pull through the mirror and retag, then
  migrate a fresh database once:
  ```bash
  for i in library/postgres:16 library/redis:7 axllent/mailpit:v1.21 stripe/stripe-mock:v0.197.0; do
    docker pull -q mirror.gcr.io/$i && docker tag mirror.gcr.io/$i ${i#library/}; done
  pnpm infra:up && pnpm db:migrate && pnpm db:seed
  ```
- The E2E runner gives the app a zsh login shell whatever `$SHELL` is (some
  containers put a real `claude` on bash's login PATH via `/etc/profile.d`,
  which E2E-M4-1 must not see).

## 4. The work loop (one slice at a time)

1. Read `BUILD_LOG.md` "Current state" and the open task in `build-kit/MILESTONES.md`.
2. Read the feature's text and acceptance criteria in `PRODUCT_SPEC.md` and its
   design in `ARCHITECTURE.md`. Where the spec leaves something open, decide,
   and record it as the next `D-n` in BUILD_LOG "Decisions".
3. New IPC command or event? Add its row to `ARCHITECTURE.md` §3 first.
4. Write the tests from the acceptance criteria first, with the ID in the name
   (`f_th_16_1_…` in Rust, `"F-TH-16.1 …"` in TypeScript).
5. Implement. Then unit tests, the milestone's E2E spec (`E2E-Mx-n` in MILESTONES),
   and run that spec.
6. Run the gate (§5). It must be green; fix causes, never weaken a test.
7. Update BUILD_LOG: tick the task with a paragraph on what was built, add any
   D-n, add a log line (what was built, and what bugs the tests found).
8. Commit `M<n>: F-XX-nn <what>` and push your branch.
9. Milestone end: the full E2E regression (every spec file), the milestone's perf
   suite where it has one, BUILD_LOG "Current state", tag.

Checklists that bite if skipped:
- **New Tauri command:** the ARCHITECTURE row; the `#[tauri::command]` in
  `src-tauri/src/commands/*`; `lib.rs` handler list; `build.rs` `COMMANDS`;
  `capabilities/main.json` `allow-<name>`; types with `#[derive(TS)] #[ts(export)]`
  then `pnpm gen:protocol` (stage the new generated files: the gate diffs
  `packages/protocol` against the index).
- **New setting:** `setting_keys!` in `crates/tandem-core/src/settings.rs` (no doc
  comments inside the macro).
- **New palette command:** the registry in `crates/tandem-core/src/commands.rs`
  (bindings must be unique), its `EnabledWhen`, and `cmd.<id>` in `en.json`. E2E-M3-9
  runs every command from the palette, so a context-only command needs that
  context set up there.
- **UI text:** every string in `apps/desktop/src/i18n/en.json` (ESLint
  `react/jsx-no-literals`). Only `@tandem/ui` components and tokens. Every screen
  has empty, loading, error and populated states.

## 5. Commands

```bash
sh scripts/agent/with-infra.sh bash scripts/gate.sh     # the gate (G): lint, all tests, desktop build, clippy, fmt, protocol drift (~20 min)
pnpm --filter desktop exec vitest run src/features/thread   # one area's TS tests
cargo test -p tandem-desktop --lib f_th_16                  # Rust tests by name
pnpm gen:protocol                                           # regenerate TS types + shortcuts.json + settings.json
pnpm gen:docs                                               # just the docs' generated sources (shortcuts, settings)

# Desktop E2E (real Tauri binary via tauri-driver; builds first, ~10 min for all 60 files)
sh scripts/agent/with-infra.sh dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" pnpm --filter desktop e2e
# one or a few specs: ONE --spec with a glob (the runner keeps only the first --spec)
... pnpm --filter desktop e2e -- --spec 'e2e/specs/m7-{8,10}-*.e2e.ts'
TANDEM_E2E_SKIP_BUILD=1 ...                                 # reuse the last build after test-only edits
# "sign-up failed: 500" in many specs after a merge: the local database lacks the
# new API migrations (relation "…" does not exist in the API's log). Run
pnpm db:migrate && pnpm db:seed                             # inside with-infra, then E2E again
# Plain xvfb-run (no session bus, small screen) fails F-SH-01.1, the TUI goldens
# and ls's column order: always use the dbus-run-session line above.

# Remote (M14): the sshd fixture, the bundled tandem-host, the SSH integration test
bash scripts/gen-ssh-fixture.sh && docker compose -f infra/docker-compose.yml --profile remote up -d --wait sshd
sh scripts/agent/build-sshd-image.sh                        # agent containers (HTTPS proxy only): build the sshd image first
sh scripts/build-tandem-host.sh                             # musl binary + manifest into src-tauri/resources/tandem-host
cargo test -p tandem-host --features e2e-ssh -- --include-ignored f_rm_01

pnpm --filter desktop perf -- --suite threads               # perf suites (threads, terminals, startup); TANDEM_PERF_BUILD=debug
pnpm --filter desktop build:demo                            # the browser demo → apps/web/public/demo/app/
pnpm --filter web build:site                                # the public site (demo included) → apps/web/out
TANDEM_HOSTED=0 bash scripts/release-local.sh --dry-run     # Linux .deb + AppImage → dist/ (builds tandem-host
                                                            # for Linux x86_64 and aarch64 first; needs cargo-zigbuild)
sh scripts/check-host-bundle.sh dist/<name>.deb             # the bundle carries tandem-host (release-local runs it)
```

E2E lessons (each cost a debugging round):
- WebDriver `getText()` returns "" for elements it thinks are hidden (fading
  dialogs): read `textContent` with `browser.execute`.
- `browser.keys()` drops a repeated letter; type with `addValue()`.
- The page's CSP blocks `new Function` inside `browser.execute`.
- Spec files run in name order (`m7-12` before `m7-2`); each starts from
  `freshOffline()` / `freshUser()`.
- The test keychain lives in memory: after `relaunch()` call `signInDesktop(email)`
  again (offline profiles survive).
- Select options include the engine glyph's letters: match the inner label node
  (`//*[@role="option"][.//*[normalize-space()="Codex"]]`).
- Toasts: `[role="region"][aria-label="Notifications"]` (the notification center
  shares the label).
- The fake engine (`crates/tandem-engines/src/bin/fake-engine`) plays Claude
  stream-json, Codex app-server and ACP. It echoes "You said: <input>" and supports
  `run <cmd>`, `write <path>=<text>;…`, `/slow N`, `/hang`, `/crash`.
- A test that hard-codes a date breaks the next day: pin the clock
  (`vi.useFakeTimers({ toFake: ["Date"] })`).
- When an E2E fails, take a screenshot (`browser.saveScreenshot`) before guessing.
- WDIO reads a test's timeout before its body runs, so `this.timeout()` inside an
  `it` or hook does nothing past `mochaOpts.timeout` (60 s): set it on the suite,
  `describe("…", function () { this.timeout(300_000); … })`.
- Sign-up or `/__test/*` answering 500 or "No user owner@tandem.test" in many
  unrelated specs means the dev database is behind: `cd apps/api && pnpm db:migrate
  && pnpm db:seed` (with the infra up) after merging a branch that adds migrations.
- Only one E2E run at a time: a killed run's `run.ts` can outlive `pkill` and
  share tauri-driver's :4444, so specs drive the other run's app. Check
  `pgrep -af "e2e/run.ts|tauri-driver|WebKitWebDriver"` before starting.
- Conductor specs (`m16-1-*`) talk to `scripts/mock-realtime.ts` on :8794 (the runner
  starts it and starts the API with `REALTIME_MOCK_URL=ws://127.0.0.1:8794/realtime`;
  an API already running from elsewhere lacks it, so stop it first). The mock's
  `/__say`, `/__state`, `/__control` and `/__close` steer it; `test_conductor_inject`
  runs doer tool calls the mock doer doesn't plan, and `test_conductor_clock` moves
  only the call's clock (the 30-minute cap).
- Dictate specs download models from `scripts/serve-models.ts` (:8792, the real
  `~/.cache/tandem-models`; run `bash scripts/fetch-models.sh base.en parakeet`
  first). The runner points `HOME` at the test's home, so anything that needs the
  real home must capture it before then.

## 6. Releasing the desktop app (Linux, local)

GitHub Actions minutes ran out (they reset monthly), so releases are built
locally; `.github/workflows/release.yml` does the same once Actions run.

1. Full E2E regression green; gate green.
2. Bump the version in `apps/desktop/src-tauri/tauri.conf.json`,
   `apps/desktop/package.json` and the root `Cargo.toml` (Cargo.lock follows).
   Add a `## x.y.z` section to `CHANGELOG.md` in its format (a summary, then `### Added` /
   `Changed` / `Fixed` / `Security`, each bullet tagged `[lin]`, `[mac]`, `[win]` or `[all]`;
   `pnpm exec tsx scripts/check-changelog.ts` checks it): only what the build really does,
   and plan limits stated (for example, the six-thread grid is Pro-only).
3. `TANDEM_HOSTED=0 bash scripts/release-local.sh --dry-run` (needs ~5 GB free;
   `TANDEM_HOSTED=0` = no hosted services, the Free plan runs locally, nothing is
   sent to our servers).
4. Smoke-test offline: `xvfb-run -a -s "-screen 0 1440x900x24" sh scripts/agent/smoke-appimage.sh`
   and look at the screenshots (welcome says "Free plan"; setup starts; the status
   bar shows the new version). Step into the new features by hand on a persistent
   display (see the script's header).
5. Site: copy `dist/Vibezeo_x.y.z_amd64-unsigned.deb` to
   `apps/web/public/downloads/Vibezeo_x.y.z_amd64.deb`, remove the old one,
   regenerate the checksums (`sha256sum Vibezeo_x.y.z_amd64.deb | tee SHA256SUMS > SHA256SUMS.txt`;
   the site links the `.txt`, which downloads under its own name: an extension-less file gets a
   trailing-slash redirect), and
   update `apps/web/lib/release.ts` (version, file names), `apps/web/lib/progress.ts`
   (milestone entries and counts taken from the gate output and the spec files) and
   the home page's feature and roadmap lists. The site tests check the file exists.
   Then the demo (D-128): every surface the demo at `/demo/app/` shows must be in
   the download being released (Thread mode with the scripted fix, Plan mode,
   the engine-switch divider, Code mode with agent panes and a shell, the
   Dashboard, Files, the git panel, the Scratchpad, the command palette), its
   plan must match the download's (`ENTITLEMENT` in `src/demo/backend.ts`), and
   anything new and visible in the release should be added to it with a test in
   `demo.test.ts`. List the surfaces in the release pull request.
6. Gate, commit, push, then deploy (§7) with the owner's go-ahead.

## 7. Deploying the website

- The site is the static export of `apps/web` (pages are `*.site.tsx`, no
  scripts on them: F-WB-01 AC2). `/demo/app/` is the browser demo (D-126): the
  desktop interface built against a simulated core in `apps/desktop/src/demo`.
- `apps/web/vercel.json` holds the install (`--filter web... --filter desktop...`),
  the build (`pnpm build:site`), headers (CSP, no framing) and the skip rule.
- Check a site change before deploying: `pnpm --filter web exec vitest run`,
  `pnpm --filter web build:site`, the home page's demo in a real browser
  (`PLAYWRIGHT_CHROMIUM=/opt/pw-browsers/chromium pnpm --filter web test:site`,
  against `apps/web/out`), Lighthouse CI on `/` and `/pricing/`
  (`CHROME_PATH=/opt/pw-browsers/chromium pnpm --filter web lhci`: 90 or more for
  performance and accessibility, three runs each, served compressed), then walk the demo:
  `(cd apps/web/public && python3 -m http.server 4321 &)` and
  `node scripts/agent/demo-walkthrough.mjs /tmp/demo-shots` (screenshots plus any
  console errors). A clean-clone build with Vercel's exact install command is the
  strongest check before touching `main`.
- **Deploy** = merge the product branch into `main` and push, only with the
  owner's go-ahead (some agent environments block pushes to `main` outright; then
  the owner merges on GitHub). Vercel builds in a few minutes.
- After deploying, verify on the live site:
  ```bash
  # Status codes with -w: through the agent proxy, `curl -sI | head -1` prints the
  # proxy's "200 Connection Established" instead.
  for p in / /demo/app/ /docs/ /downloads/Vibezeo_x.y.z_amd64.deb /downloads/SHA256SUMS.txt \
    /llms.txt /llms-full.txt /openapi.json /api/v1/site-info/; do
    echo "$p $(curl -s -o /dev/null -w '%{http_code}' https://vibezeo.com$p)"; done
  # /api/v1/site-info answers 308 to its trailing-slash form, which is the JSON (D-135).
  curl -sL https://vibezeo.com/api/v1/site-info | grep -o '"version": "[0-9.]*"'
  curl -sI https://vibezeo.com/ | grep -iE "^(content-security-policy|x-frame-options)"
  # React splits the text with <!-- -->: strip it before matching.
  curl -s https://vibezeo.com/ | sed 's/<!-- -->//g' | grep -o "Released · [0-9.]*"
  curl -s https://vibezeo.com/downloads/SHA256SUMS.txt
  ```
  and the deployment's status on the `main` commit (GitHub commit status from
  Vercel, or the Vercel dashboard).

## 8. Monitoring

Today there's no production backend: the API isn't deployed and the desktop
release runs offline. What to watch:
- **The site:** the curl checks above; Vercel deployment status after each push to
  `main`; Google Search Console (Pages and Sitemaps) and Bing Webmaster Tools.
- **Mail:** `hello@vibezeo.com` (contact address on every page and in `security.txt`).
- **CI:** GitHub Actions when minutes are available (CI runs Linux on pull requests;
  macOS and Windows on `main`).
- **Later** (when the API ships, see `HUMAN_TODO.md`): `GET /v1/health`, the API's
  pino logs (tokens and content are redacted), and opt-in-only crash and usage
  reports (Sentry, PostHog; nothing is sent unless the user turns it on).

## 9. Non-negotiables

- Everything in `CLAUDE.md` "Never": no other products' names, branding or code;
  secrets only in the OS keychain; never log tokens, prompts or file contents;
  nothing leaves the machine without the user's opt-in; never read, copy or proxy
  AI vendors' OAuth tokens; never skip or weaken a failing test; never block on a
  person (stub it, add it to `HUMAN_TODO.md`, continue).
- **Every public claim must be true and checkable.** No invented customers,
  revenue, users, quotes, dates or reviews; no feature the shipped build doesn't
  have; plan limits stated where they apply; demos and illustrations labelled.
  Reviewers of applications and listings check claims, and one false claim costs
  more than it gains: refuse those requests and offer the honest version.
- Ask the owner before anything outward-facing that's hard to undo: deploying,
  publishing a release, deleting data, changing hosting or DNS.
