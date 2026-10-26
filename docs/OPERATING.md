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
| macOS: checks from Linux, Mac CI, signing, notarization, distribution | `docs/MACOS.md` |
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
                                                 # openssh-client (ssh, ssh-keygen, ssh-keyscan), musl-tools, bubblewrap, …
apt-get install -y zsh fish                      # the shell-integration E2E specs need them
apt-get install -y bubblewrap                    # E2E-M18-TL11 and the real-sandbox Rust tests need bwrap (F-TL-11)
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl  # tandem-host (M14)
pip install ziglang==0.13.0 && cargo install cargo-zigbuild --locked     # release-local.sh: the aarch64 helper
cargo install cargo-audit --locked
cargo install cargo-about --locked --features cli # release-local.sh: notices
cargo install cargo-cyclonedx --locked           # release-local.sh: SBOMs
apt-get install -y rpm rpm2cpio cpio            # release-local.sh: reads the .rpm (D-832)
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
  When the gate runs short, delete the test binaries
  (`find target/debug/deps -maxdepth 1 -type f -executable -size +20M ! -name '*.so' -delete`)
  once `cargo test` has passed, and again after `pnpm gen:protocol`, which builds
  the crates' tests again to export the bindings. A cross target can go once its
  check has passed. A container restart stops background runs, so give long ones
  a marker per finished step and skip marked steps when they run again.
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
# E2E prerequisites beyond bootstrap-linux.sh: zsh and fish (shell specs) and
# bubblewrap (E2E-M18-TL11 fails at once with "needs bubblewrap" without it; the
# sandbox also needs unprivileged user namespaces, so where bwrap can't run, the
# spec fails on sandbox_status's reason). Specs share one TANDEM_HOME in file
# order: a spec that leaves panes open closes them in an `after` hook, or the next
# launch shows the "still running" dialog (closeAllPanes, stopLeftovers).

# Remote (M14): the sshd fixture, the bundled tandem-host, the SSH integration test
bash scripts/gen-ssh-fixture.sh && docker compose -f infra/docker-compose.yml --profile remote up -d --wait sshd
sh scripts/agent/build-sshd-image.sh                        # agent containers (HTTPS proxy only): build the sshd image first
sh scripts/build-tandem-host.sh                             # musl binary + manifest into src-tauri/resources/tandem-host
cargo test -p tandem-host --features e2e-ssh -- --include-ignored f_rm_01

pnpm --filter desktop perf -- --suite threads               # perf suites (threads, terminals, startup); TANDEM_PERF_BUILD=debug
pnpm --filter desktop build:demo                            # the browser demo → apps/web/public/demo/app/
pnpm --filter web build:site                                # the public site (demo included) → apps/web/out
TANDEM_HOSTED=0 bash scripts/release-local.sh --dry-run     # Linux .deb, .rpm + AppImage → dist/ (builds tandem-host
                                                            # for Linux x86_64 and aarch64 first; needs cargo-zigbuild)
sh scripts/check-host-bundle.sh dist/<name>.deb             # the bundle carries tandem-host (.deb, .rpm or AppImage;
                                                            # release-local runs it)
sh scripts/agent/test-rpm-fedora.sh dist/<name>.rpm 43      # the .rpm installs, finds its libraries and starts in a
                                                            # clean Fedora container (D-832); the site names only the
                                                            # Fedora releases it passes on
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
  `run <cmd>`, `write <path>=<text>;…`, `/slow N`, `/hang`, `/crash`. All three
  answer a task review with a verdict (`review_reply`), since a review runs on
  whichever other chat engine is installed.
- A spec that puts a fake engine in `$TANDEM_HOME/fakebin` removes it in `after`
  and calls `checkAgain()`, or later specs run with it installed: a leftover
  Cursor Agent reviewed E2E-M18-TK-07's runs in a full run, though it passed alone.
- `waitUntil`'s `timeoutMsg` is built with its options, before the wait, so
  `` `…${JSON.stringify(await rows())}` `` shows the state at the start. Keep what
  the condition last read and throw it when the wait times out.
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
- Conductor's free primary against Google's real Gemini Live (D-1018):
  `GEMINI_API_KEY=… bash scripts/agent/gemini-live-check.sh` (about a minute;
  `--long` adds a 12-minute call past Google's 10-minute reconnect). The API's
  `GeminiLive` mints each token and the app's own client opens it; tokens stay in
  a private temp file and are never printed. In a cloud session, add
  `GEMINI_API_KEY` in the environment's settings and start a new session (a
  running one doesn't see it). The WebSocket goes out directly, which works
  from Claude's cloud containers.
- Dictate specs download models from `scripts/serve-models.ts` (:8792, the real
  `~/.cache/tandem-models`; run `bash scripts/fetch-models.sh base.en parakeet`
  first). The runner points `HOME` at the test's home, so anything that needs the
  real home must capture it before then.
- The wake phrase (F-CN-07) runs in the `tandem-wake` sidecar, built with
  `cargo build -p tandem-voice --features kws --bin tandem-wake` (release
  builds: `scripts/build-sidecars.sh`). Behind a TLS-intercepting proxy, run
  `bash scripts/fetch-sherpa.sh` first: `sherpa-onnx-sys` can't verify the
  proxy's CA, so it reads the archive from `SHERPA_ONNX_ARCHIVE_DIR` (the gate
  sets it). Its benchmark: `cargo test -p tandem-voice --features kws --
  --include-ignored f_cn_07_1` (about a minute).
- `ort-sys` (ONNX Runtime for Dictate's Parakeet: tandem-voice's `parakeet`
  feature, which the app turns on) downloads its library from `cdn.pyke.io`
  when it builds. A cloud environment whose network policy blocks that host
  can't build the app or the invariants, so the gate's Rust half can't run
  there: add the host under the environment's Allowed domains. Type-checking
  still works without it: `ORT_SKIP_DOWNLOAD=1 cargo clippy --workspace
  --all-targets -- -D warnings` (ort-sys's own switch: no download is tried,
  and the missing library only fails a link). Tests and builds of those two
  crates still need the host.

### 5a. CI and its minutes

GitHub gives this private repository 2,000 free Actions minutes a month, and a
macOS minute counts as 10, a Windows minute as 2. October's ran out in a week
(75–100 Linux minutes a pull request, 250–350 a push to `main`), and the owner
doesn't pay for more. So `ci.yml` runs on its own only what a fresh machine
adds to the gate (D-1009):

| When | Job | What | About |
|---|---|---|---|
| A pull request into `product-m0-m5` (not a draft, not docs only) | `quick` | spec coverage, the hook guard, rustfmt, and every package's typecheck and ESLint (`pnpm -r lint`); nothing compiled, no tests | 5 min |
| Every night at 21:37 UTC | `checks` | the gate's Linux half on `product-m0-m5`'s head (TypeScript, `cargo test`, clippy, fmt, protocol drift) and the audit. A commit that was already checked is skipped | 25 min |
| By hand (Actions › CI › Run workflow, any branch) | `checks` | the same, on that branch | 25 min |
| … with **full** | `web`, `desktop-build`, `integration`, `tandem-host`, the Whisper test | the site's Playwright tests, the debug app and the release harness, the desktop E2E, the SSH helpers | +60 min |
| … with **platforms** | `test-rust`, `desktop-build`, `tandem-host`, `e2e-windows` | the same on macOS and Windows | +500 counted |

At October's pace (100–150 pull requests a month) that's about 500–750
minutes of `quick` and at most 750 of `checks`. That leaves a few hundred for
runs by hand, and one with **platforms** spends a quarter of the month. These are
estimates from October's runs: check Settings › Billing and licensing after
November 1 and adjust. Pull requests here are merged seconds after they open,
so no run ever gated a merge. The gate does that, before every commit, and
everything CI runs also runs locally:
- `pnpm gate` covers `quick` and `checks`, plus the Windows and macOS clippy.
- `pnpm ci:local` adds the Wine tests, the audit and the desktop E2E
  (`TANDEM_CI_INTEGRATION=0` skips the E2E).
- The rest of **full**: `bash scripts/check-release-harness.sh` (a release
  build), the site's Playwright tests (`PLAYWRIGHT_CHROMIUM=/opt/pw-browsers/chromium
  pnpm --filter web test:e2e` after `db:migrate`, `db:seed` and `keys:dev`) and
  `sh scripts/build-tandem-host.sh`.

## 6. Releasing the desktop app (Linux, local)

GitHub Actions minutes ran out (they reset monthly), so releases are built
locally; `.github/workflows/release.yml` does the same once Actions run.

1. Full E2E regression green; gate green.
2. Bump the version in `apps/desktop/src-tauri/tauri.conf.json`,
   `apps/desktop/package.json` and the root `Cargo.toml` (Cargo.lock follows).
   Rename `CHANGELOG.md`'s `## Unreleased` section (where slices gather their notes,
   D-2906) to `## x.y.z`, or add one, in its format (a summary, then `### Added` /
   `Changed` / `Fixed` / `Security`, each bullet tagged `[lin]`, `[mac]`, `[win]` or `[all]`;
   `pnpm exec tsx scripts/check-changelog.ts` checks it): only what the build really does,
   and what needs a Vibezeo account stated (Vibezeo is free, D-869: nothing local is limited).
3. `TANDEM_HOSTED=0 bash scripts/release-local.sh --dry-run` (needs ~5 GB free;
   `TANDEM_HOSTED=0` = no hosted services, everything local runs free and unlimited,
   nothing is sent to our servers).
4. Smoke-test offline: `xvfb-run -a -s "-screen 0 1440x900x24" sh scripts/agent/smoke-appimage.sh`
   and look at the screenshots (welcome says "Free · no account · nothing sent to Vibezeo"; setup starts; the status
   bar shows the new version). Step into the new features by hand on a persistent
   display (see the script's header).
5. Site: copy `dist/Vibezeo_x.y.z_amd64-unsigned.deb` to
   `apps/web/public/downloads/Vibezeo_x.y.z_amd64.deb`, remove the old one,
   regenerate the checksums (`sha256sum Vibezeo_x.y.z_amd64.deb | tee SHA256SUMS > SHA256SUMS.txt`;
   the site links the `.txt`, which downloads under its own name: an extension-less file gets a
   trailing-slash redirect), and
   update `apps/web/lib/release.ts` (version, file names), `apps/web/lib/progress.ts`
   (milestone entries and counts taken from the gate output and the spec files) and
   the home page's feature and roadmap lists (`apps/web/lib/home.ts`; the "New in"
   tiles come from the CHANGELOG, and a new Added title needs an entry in
   `RELEASE_TILE_MAP`). The site tests check the file exists. Captures are taken
   at 2× and kept as masters in `apps/web/media/<name>@2x.png` (D-1838): the demo
   still from `/demo/app/?embed=1` in Chromium at 1280×800 with device scale 2, and
   the app's screens from the real build under the E2E harness with `GDK_SCALE=2`
   (hide the debug build's DEV badge; no agents, or the fake engine labelled). Then
   run `python3 apps/web/scripts/optimize-screens.py`, which writes the 1× original
   and AVIF and WebP at 1× and 2×, and set each screen's `build` in `SCREENS`
   (`apps/web/lib/media.ts`). If the video is re-recorded, set
   `VIDEO.recordedWith` there too.
   Then the demo (D-128): every surface the demo at `/demo/app/` shows must be in
   the download being released (Thread mode with the scripted fix, Plan mode,
   the engine-switch divider, Code mode with agent panes and a shell, the
   Dashboard, Files, the git panel, the Scratchpad, the command palette), its
   plan must match the download's (`ENTITLEMENT` in `src/demo/backend.ts`), and
   anything new and visible in the release should be added to it with a test in
   `demo.test.ts`. List the surfaces in the release pull request.
   The home page's product scenes (D-1880, `components/site/scenes`) follow
   the same rule: they draw only what the download does, with the app's own
   words from `en.json`, and label the build from `lib/release.ts`. A scene
   or part of one that needs a newer release is gated on it (`LANDS` in
   `lib/scenes.ts` switches on 0.9.0's landing queue and intent radar): after
   bumping `release.ts`, look at each scene's final frame in both themes and
   check it against the release.
6. Gate, commit, push, then deploy (§7) with the owner's go-ahead.

### 6a. Windows from Linux (no Windows CI until Actions minutes return)

Nothing here has run on a real Windows machine; HUMAN_TODO's "M15 Windows first
run" has the steps, and Windows stays "Coming soon" on the site until they pass.

One-time setup (on top of §3):
```bash
apt-get install -y mingw-w64 nsis p7zip-full clang-19 lld-19 llvm-19
rustup target add x86_64-pc-windows-gnu x86_64-pc-windows-msvc
cargo install cargo-xwin --locked        # downloads the MSVC CRT and SDK on first use (~1 GB in ~/.cache/cargo-xwin):
                                         # only after the owner accepts Microsoft's licence (HUMAN_TODO, D-1711);
                                         # then export TANDEM_MSVC_LICENSE_ACCEPTED=1
# Wine 11 from WineHQ (Ubuntu's Wine 9 also runs the tests):
mkdir -p /etc/apt/keyrings
curl -fsSL https://dl.winehq.org/wine-builds/winehq.key -o /etc/apt/keyrings/winehq-archive.key
curl -fsSL https://dl.winehq.org/wine-builds/ubuntu/dists/noble/winehq-noble.sources -o /etc/apt/sources.list.d/winehq-noble.sources
apt-get update && apt-get install -y --install-recommends winehq-stable
```

| Command | What it checks | Time (4 vCPU) |
|---|---|---|
| `bash scripts/windows-check.sh --clippy` | `cfg(windows)` code in every crate but the app, `x86_64-pc-windows-gnu`. In the gate. | ~40 s warm |
| `bash scripts/windows-check.sh --msvc` | The whole workspace, app included, for `x86_64-pc-windows-msvc` through cargo-xwin. In `ci:local` once the licence is accepted. | ~70 s warm, ~4 min cold |
| `bash scripts/windows-check.sh --wine` | tandem-pty, tandem-core, tandem-routines, tandem-voice and tandem-host tests under Wine. In `ci:local`. | ~75 s warm |
| `bash scripts/build-windows-installer.sh` | Needs the licence too. Unsigned NSIS installer, then `scripts/check-windows-bundle.sh` on it. | ~20 min cold, the app 11 min of it |

- The app isn't in the GNU check: ONNX Runtime has no prebuilt MinGW binaries and
  whisper.cpp doesn't compile against Ubuntu's mingw-w64 11 headers. The MSVC
  build needs clang 19 or later (the MSVC STL headers refuse 18); the scripts put
  `/usr/lib/llvm-19/bin` first on PATH.
- What Wine can't run (`windows-check.sh` says why for each): ConPTY (Wine's
  conhost rejects `--inheritcursor` and exits, so `tests/pty_windows.rs` waits for
  real Windows), recursive file watching of folders that existed before the
  watch, and the git-backed tandem-host tests.
- The installer: `target/x86_64-pc-windows-msvc/release/bundle/nsis/Vibezeo_<version>_x64-setup.exe`.
  Only NSIS cross-builds; the MSI needs WiX on Windows (release.yml builds both).
  `TANDEM_HOSTED=0` as for Linux releases. `--skip-host` reuses the tandem-host
  helpers already in `resources/tandem-host` (the full run builds the Linux ones
  with cargo-zigbuild, §3). Don't publish it.
- `scripts/check-windows-bundle.sh <setup.exe>` lists the installer with 7-Zip and
  fails unless the app, `tandem-tasks.exe`, `tandem-hook.exe`, `DirectML.dll` and the tandem-host
  helpers are in it, and checks the generated `installer.nsi` for the Start Menu
  shortcut's AppUserModelID (`com.vibezeo.tandem`, which toasts use), the
  `tandem://` scheme and the WebView2 bootstrapper.
- Disk: the MSVC release build takes ~2.5 GB under `target/x86_64-pc-windows-msvc`
  and the Wine test binaries ~3 GB under `target/x86_64-pc-windows-gnu`; delete
  them after use when space is short.

### 6b. Microsoft Store (MSIX)

The Store signs, hosts and updates MSIX packages for free (D-1713). The package
holds the same files as the NSIS installer (the app, `tandem-tasks.exe`,
`tandem-hook.exe`, `DirectML.dll`, the tandem-host helpers) plus
`packaging/msix/AppxManifest.xml.in`: a full-trust app (`runFullTrust`), the
`tandem://` protocol, the `vibezeo.exe` execution alias (Task Scheduler runs
routines through it) and the microphone.

```bash
bash scripts/get-makemsix.sh                       # once: builds makemsix (MIT) from a pinned commit (~6 min; cmake ≥ 3.29: pip install cmake)
TANDEM_MSVC_LICENSE_ACCEPTED=1 bash scripts/build-windows-installer.sh   # the binaries (D-1711)
MSIX_IDENTITY_NAME=… MSIX_PUBLISHER='CN=…' MSIX_PUBLISHER_DISPLAY_NAME=… \
  bash scripts/build-msix.sh --from-installer target/x86_64-pc-windows-msvc/release/bundle/nsis/Vibezeo_<v>_x64-setup.exe
# → dist/Vibezeo_<v>_x64.msix, then scripts/check-msix.sh on it
```

- The three `MSIX_*` values come from Partner Center (Product management ›
  Product identity) once the name is reserved; without them the package gets dev
  values that a Store upload refuses. Version is `tauri.conf.json`'s plus `.0`.
- A Store install never runs the app's own updater (`tandem_core::install`).
- To try a package on Windows without the Store: unzip it, then in PowerShell with
  Developer Mode on, `Add-AppxPackage -Register .\AppxManifest.xml`.

### 6c. Windows signing, winget and E2E (when Actions minutes exist)

- **Trusted Signing (D-1715).** With the `AZURE_*` secrets set, release.yml installs
  `trusted-signing-cli` on the Windows runner and passes it to Tauri as the
  `signCommand`, which signs the app, both sidecars and the NSIS and MSI
  installers. Without them the artifacts are marked `-unsigned`.
- **MSIX.** The `msix` job packs the Store package from the Windows installer
  (§6b) into the `store-msix` artifact; it never goes into the public release.
- **winget (D-1715).** For a signed early-access release, `publish` writes the
  manifests (`scripts/gen-winget.sh`, validated against `packaging/winget/schemas`)
  into the `winget-manifests` artifact. By hand:
  `bash scripts/gen-winget.sh <setup.exe> <its public https URL>` (needs
  `pip install pyyaml jsonschema`).
- **Windows E2E (D-1714).** `ci.yml`'s `e2e-windows` job (by hand, with
  **platforms**, §5a) installs tauri-driver, fetches the `msedgedriver` matching the runner's
  WebView2, and runs `pnpm --filter desktop e2e -- --spec-list e2e/windows-smoke.txt`
  with `TANDEM_E2E_OFFLINE=1` (no API, web app or mock servers: they need Docker).
  On Linux the same command checks the list:
  `TANDEM_E2E_OFFLINE=1 dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" pnpm --filter desktop e2e -- --spec-list e2e/windows-smoke.txt`.

## 7. Deploying the website

- The site is the static export of `apps/web` (pages are `*.site.tsx`, no
  scripts on them: F-WB-01 AC2). `/demo/app/` is the browser demo (D-126): the
  desktop interface built against a simulated core in `apps/desktop/src/demo`.
- `apps/web/vercel.json` holds the install (`--filter web... --filter desktop...`),
  the build (`pnpm build:site`), headers (CSP, no framing) and the skip rule.
- Check a site change before deploying: `pnpm --filter web exec vitest run`,
  `pnpm --filter web build:site`, the home page's demo in a real browser
  (`PLAYWRIGHT_CHROMIUM=/opt/pw-browsers/chromium pnpm --filter web test:site`,
  against `apps/web/out`), Lighthouse CI on `/` and `/download/`
  (`CHROME_PATH=/opt/pw-browsers/chromium pnpm --filter web lhci`: `/`, `/about/`,
  `/download/`, `/changelog/`, `/docs/`, `/blog/` and one post, three runs each, 95 or more in all four
  categories and CLS under 0.02; delete `apps/web/.lighthouseci` first, it keeps old
  runs; `pnpm --filter web lhci:dark` runs the same in the dark theme, D-1870), then walk the demo:
  `(cd apps/web/public && python3 -m http.server 4321 &)` and
  `node scripts/agent/demo-walkthrough.mjs /tmp/demo-shots` (screenshots plus any
  console errors). A clean-clone build with Vercel's exact install command is the
  strongest check before touching `main`.
- **Deployment limit (D-829):** Vercel's plan caps deployments per day, and every push to a branch Vercel deploys counts. Only `main` deploys (`git.deploymentEnabled`). Batch: merge features into `product-m0-m5` as they're ready, and move `main` only for a release that bundles them all. One release is one deployment. Never merge into `main` for a single small change unless it's a hotfix the live site needs. If Vercel answers "Deployment rate limited", wait it out (the limit is a rolling day) or use **Redeploy** on the latest `main` deployment once it clears.
- `build:site` ends with `scripts/strip-runtime.mjs` (D-1813): it removes Next's
  unused client runtime from the marketing pages and the docs (not the demo app;
  D-1843)
  and fails the build if any page then has a script beyond `/demo/embed.js` on `/`
  and `/changelog/0.9.0/` (the Land scene, D-1885), `/download/detect.js` on
  `/download/` and a blog post's two (`scripts/script-allowlist.mjs`). Site components stay server-only.
  Then `scripts/check-routes.mjs` (D-1815) checks every page answers at its
  trailing-slash URL with no redirect: Next's own trailing-slash redirects are
  off for the export, and the "add a slash" rule is in `vercel.json`.
- **Blog posts** (F-WB-06) are files in `apps/web/content/blog`; `docs/BLOG.md` §3 is the owner's how-to. They deploy like any other site change.
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
- **Mail:** `contact@vibezeo.com` (contact address on every page and in `security.txt`).
- **CI:** GitHub Actions when minutes are available: `quick` on pull requests, the
  nightly `checks`, and the rest by hand (§5a).
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
  have; what needs a Vibezeo account stated where it applies; demos and illustrations labelled.
  Reviewers of applications and listings check claims, and one false claim costs
  more than it gains: refuse those requests and offer the honest version.
- Ask the owner before anything outward-facing that's hard to undo: deploying,
  publishing a release, deleting data, changing hosting or DNS.
