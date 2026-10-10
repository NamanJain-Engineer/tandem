# Vibezeo on macOS: building, signing and shipping

The macOS app is the same product as the Linux one (PLATFORMS_PLAN §4.1): a
universal `.app` and `.dmg` for macOS 14 or later, signed with a Developer
ID, notarized and stapled. **Nothing here has run on a Mac yet** (no Mac in
the agent containers, no Actions minutes until about November 1).
HUMAN_TODO's "macOS first run" lists the steps that need one, and the site
keeps macOS at "Coming soon" until they pass. Decisions: D-2100…D-2199 in
BUILD_LOG ("M15 · macOS from Linux").

## 1. What's in the repository

| File | What it does |
|---|---|
| `apps/desktop/src-tauri/tauri.macos.conf.json` | Merged by Tauri on macOS only: the overlay title bar, `minimumSystemVersion` 14.0, the hardened runtime, `Entitlements.plist`, the dmg's layout (D-2100). |
| `apps/desktop/src-tauri/Info.plist` | Merged into the app's Info.plist: `NSMicrophoneUsageDescription` for Dictate (D-2101). |
| `apps/desktop/src-tauri/Entitlements.plist` | One entitlement, `com.apple.security.device.audio-input`; the file says why each other one is absent (D-2101). |
| `scripts/ci/macos-sign.sh` | `identity` (a throwaway keychain from `APPLE_CERTIFICATE`, or ad-hoc), `sign` (hardened runtime, secure timestamp), `notarize` (notarytool, then staple), `verify` (every Mach-O universal, signed, hardened, timestamped, one team; Gatekeeper with `--notarized`). |
| `scripts/ci/macos-release.sh` | The whole macOS release on one Mac (Codemagic, a self-hosted Mac): helpers, sidecars, app, dmg, notarization, checks, `out/` with SHA256SUMS (D-2104). |
| `scripts/ci/macos-smoke.sh` | Launches a built `.app` with a fresh `TANDEM_HOME`: Info.plist keys, `--run-routines` exits 0, the app stays up 25 s with no panic, a screenshot (D-2103). |
| `scripts/macos-check.sh` | `cargo check`/`clippy` of the whole workspace for both Mac architectures from Linux (D-2105). In the gate. |
| `.github/workflows/release.yml` | Tags build every OS; its macOS path signs `tandem-host` in its own job, signs the sidecars, lets Tauri sign and notarize the app, then notarizes the dmg and verifies (D-2102). |
| `.github/workflows/macos.yml` | By hand: macOS clippy, `cargo test --workspace`, a debug app with the smoke test, and optionally the universal release path without secrets (D-2103). |
| `codemagic.yaml` | The same two jobs on Codemagic's free Mac minutes (D-2104). |
| `docs/macos/vibezeo.rb` | The Homebrew cask, for a tap, once a release is notarized (D-2107). |

## 2. Checking macOS code from Linux

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
bash scripts/macos-check.sh --clippy        # both architectures; in the gate
bash scripts/macos-check.sh --target aarch64-apple-darwin   # check only, one architecture
```

`cargo check` and `clippy` don't link, so Apple's SDK isn't needed, given three
stand-ins that don't change which Rust is checked: C and C++ sources compile to
empty objects, whisper-rs uses its bundled bindings, and the app is checked
without its `os-audio` feature. cpal's CoreAudio bindings are generated from the
SDK's headers, and Apple's licence ties the SDK to Apple hardware, so we don't
fetch it here.

| Crate | aarch64 | x86_64 | Not checked here |
|---|---|---|---|
| tandem-core, -store, -pty, -routines, -git, -engines, -gateway, -tasks, -host | clippy clean | clippy clean | Their C code (SQLite, zlib, libgit2, ring) isn't compiled for macOS. |
| tandem-voice (`whisper`, `parakeet`, no `cpal`) | clippy clean | clippy clean | `capture_cpal.rs` and the cpal chime player (cross-platform cpal code, compiled on Linux); whisper.cpp and ONNX Runtime themselves. |
| tandem-desktop (no `os-audio`) | clippy clean | clippy clean | The two `os-audio` functions in `services/dictate.rs`. |
| tandem-invariants | no | no | It dev-depends on the app with default features (cpal); it has no macOS code. |

Nothing is linked or run: linking, `objc_msgSend` signatures, framework calls
and the tests run on a Mac (`macos.yml` or `codemagic.yaml`).

## 3. Building on a Mac

- **GitHub Actions** (after about November 1, or on a self-hosted runner
  labelled `macOS`): Actions › macOS › Run workflow. `runner` picks the image or
  `self-hosted`; `universal` adds the release path without secrets (about 30
  minutes more). Releases: push a tag `v<version>` or `v<version>-early-access`
  (release.yml).
- **Codemagic** (500 free Apple-silicon minutes a month): connect the
  repository, create the environment groups `vibezeo_apple` and
  `vibezeo_updater` (codemagic.yaml lists their variables), then start
  `macos-tests` or `macos-release` by hand. Artifacts: `out/` (dmg, updater
  archive and signature, `macos-signature-*.txt`, SHA256SUMS) and `smoke/`.
- **The owner's Mac**: Xcode command line tools, rustup with both Apple
  targets, Node 22 with pnpm, then `pnpm install` and
  `bash scripts/ci/macos-release.sh` (add `cargo install cargo-zigbuild
  cargo-about --locked` and `pip3 install ziglang==0.13.0` for the Linux
  helpers and the notices). Without `APPLE_CERTIFICATE` the build is ad-hoc
  signed and named `-unsigned`.

## 4. Signing and notarization

Secrets (GitHub repository secrets, or Codemagic's `vibezeo_apple` group):

| Secret | What |
|---|---|
| `APPLE_CERTIFICATE` | The Developer ID Application certificate and key, exported as .p12, base64 (`base64 -i cert.p12 \| pbcopy`). |
| `APPLE_CERTIFICATE_PASSWORD` | The .p12's password. |
| `APPLE_SIGNING_IDENTITY` | Optional: `Developer ID Application: <name> (<team id>)`; found in the keychain when unset. |
| `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_P8` | Notarization with an App Store Connect API key (preferred): issuer id, key id, the .p8 file's text. |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Or notarization with an Apple ID and an app-specific password. `APPLE_TEAM_ID` is also checked against every signature. |

What happens, in order (release.yml and macos-release.sh alike):
1. `tandem-host` for macOS is built universal and signed **before** its SHA-256
   goes into `host-binaries/manifest.json`: signing changes the file, and Tauri
   copies resources without signing them.
2. The sidecars `tandem-tasks` and `tandem-hook` are built universal and signed.
3. Tauri builds the universal `.app`, signs it with `Entitlements.plist` and the
   hardened runtime, notarizes it and staples the ticket; the dmg holds the
   stapled app, and the updater's `.app.tar.gz` is made from it.
4. The dmg is signed, notarized and stapled, so it opens offline.
5. `macos-sign.sh verify` checks every Mach-O in the app (universal, signed,
   hardened runtime, timestamp, `APPLE_TEAM_ID`), `codesign --verify --deep
   --strict`, `spctl -a -vv` on the app and the dmg, and `stapler validate`; its
   output ships as `macos-signature.txt`, the evidence the site's release gate
   asks for. `check-host-bundle.sh` checks the helpers' hashes inside the app.

Without the certificate everything is ad-hoc signed (Apple silicon runs nothing
unsigned), the artifacts are named `-unsigned`, and users must allow the app
in System Settings › Privacy & Security › Open Anyway (macOS 15 removed
Control-click › Open).

## 5. Distribution

- **Developer ID and notarization** is the path (D-2106): direct downloads from
  vibezeo.com and the public `tandem-releases` repository, updates through
  `tauri-plugin-updater` and the `.app.tar.gz`.
- **Homebrew** (D-2107): `docs/macos/vibezeo.rb` is the cask. Put it in a tap
  (`NamanJain-Engineer/homebrew-tap`, `Casks/vibezeo.rb`) once a notarized
  release exists; per release set `version` and `sha256` from SHA256SUMS. Users:
  `brew install --cask namanjain-engineer/tap/vibezeo`. `auto_updates true`
  because the app updates itself.
- **Mac App Store: not now** (D-2106). The store requires the App Sandbox, and
  the sandbox is incompatible with what the product is:
  - terminals spawn the user's login shell, which runs their rc files and any
    program in any folder; a sandboxed child inherits the sandbox, so `git`,
    `npm`, compilers and the agent CLIs couldn't read or write the user's
    repositories outside a folder picked in an open panel, nor their dotfiles
    (`~/.zshrc`, `~/.gitconfig`, `~/.ssh`);
  - agent CLIs are installed by the user (Homebrew, npm, nvm), not shipped in
    the bundle, and the store forbids running downloaded code;
  - Dictate posts ⌘V into other apps through Accessibility and listens to the
    keyboard through an event tap; sandboxed apps can't be trusted for
    Accessibility;
  - background routines install a LaunchAgent, and the helper is uploaded to
    SSH hosts with the user's `ssh` and keys;
  - provider homes live in `~/.tandem/providers/` and engines authenticate in
    them (CLAUDE.md), outside any container.
  A store edition would have to be a different product (a thread client with
  no terminals, or engines only on remote hosts). Revisit only if that product
  is wanted; PRODUCT_SPEC §6 item 8 already rules stores out.

## 6. macOS-only features (reviewed against the code)

| Feature | Code | Platform trait and fakes | What only a Mac shows |
|---|---|---|---|
| Background routines (F-RT-03) | `crates/tandem-routines/src/launcher.rs` `LaunchdLauncher` | `OsLauncher` with `RecordingRunner` (commands) and a temp home (files) | `launchctl bootstrap gui/<uid>` loads it; System Settings › Login Items › Allow in the Background names Vibezeo. |
| Overlay title bar | `tauri.macos.conf.json`, `features/shell/WindowControls.tsx` | The window config is pinned by a test | The traffic lights at (14, 20) clear the left cluster. |
| GUI PATH (F-EN-01) | `crates/tandem-pty/src/env.rs`, `crates/tandem-engines/src/detect.rs` | Fake shells under a temp home | A Dock-launched app finds engines on the zsh login PATH. |
| Dictate permissions (F-DI-02, F-DI-03, F-DI-06) | `crates/tandem-voice/src/hotkey/macos.rs`, `insert/macos.rs`, `apps/desktop/src-tauri/src/commands/dictate.rs` | `HotkeyBackend` and `Platform` with fakes | The prompts, System Settings' lists, Quit & Reopen. |

HUMAN_TODO's "macOS first run" has the manual steps for each.
