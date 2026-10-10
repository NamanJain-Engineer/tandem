# Vibezeo on macOS, Windows, Android and iOS: platform plan

Prepared 10 October 2026 for the owner and the coordinating session.

This is research only. Nothing in `/home/user/tandem` was changed. The plan reads the repository at `release-0.5.0` (ebbe472), `origin/product-m0-m5` and `origin/m16c-phone-companion` (37c1561). It also uses the GitHub Actions run history and the official Tauri, Apple, Google, Microsoft and GitHub documentation. Sources are listed in §10.

Paths are relative to the repository root. Decision numbers for new work are proposals; the coordinator assigns the ranges (§8.4).

---

## 1. The answer in brief

| Platform | What "the same product" means | State today | What blocks shipping |
|---|---|---|---|
| **macOS** | The full desktop app: universal, macOS 14+ (DECISIONS). | The platform code is written. It has never been compiled into a binary or run on a Mac. No macOS CI job has ever started. | 1. A Mac build host. 2. An Apple Developer ID and notarization ($99 a year). 3. Three config gaps that break Dictate or notarization (§2.4). 4. A manual QA pass on a real Mac. |
| **Windows** | The full desktop app: x64, Windows 10 22H2+ and 11, with WSL workspaces. | The code is written, and parts were type-checked from Linux. It has never been built or run. Two real gaps: the registry PATH merge is missing, and SSH workspaces rely on `ControlMaster`, which Windows OpenSSH doesn't implement. | 1. A Windows host for tests (an unsigned NSIS installer can be cross-built in this container). 2. Code signing: Microsoft's Artifact Signing is open only to individuals in the US or Canada, or to organizations in 12 regions. Unsigned builds show SmartScreen. 3. A manual QA pass. |
| **Android** | Not the desktop app. A phone can't run the coding CLIs. The product is **Vibezeo Remote**, the companion to the user's own desktop: agents' status, approvals, notifications, chat with threads and teammates, routines and typing into agent panes, over the end-to-end encrypted relay. | The companion exists as a PWA on the unmerged `m16c-phone-companion` branch, with E2E tests. There's no native app. | 1. The owner must change the spec: PRODUCT_SPEC §6 rules out native apps (item 3) and store distribution (item 8). 2. The hosted API (accounts, Pro, the relay) isn't deployed, so nobody can pair today. 3. Push notifications need native code and a push relay. 4. Google Play makes a new personal account run a 12-tester, 14-day closed test first. |
| **iOS** | The same companion. | The same as Android. | Everything Android needs, plus a Mac with Xcode 26 for every build, Apple membership, and an App Review demo path (guideline 2.1). |

**Mobile recommendation.** Build a Tauri 2 app that reuses the PWA's screens, moved into a shared package. The app runs the pairing protocol in Rust (`tandem_core::remote`), which is already pinned byte-for-byte to the TypeScript version by shared test vectors. Keys go in the iOS Keychain and the Android Keystore, not in IndexedDB. Add a QR scanner, app links and push notifications. Don't port the desktop React UI to phones (§4.3).

Ship the existing PWA first. It works on both phones as soon as the API is live. The native apps follow.

**What vibezeo.com can offer, and when.** Each entry is gated on a build that exists and has been tested (§8, stream R1):

| Download | Earliest honest point |
|---|---|
| Linux .deb | Now (0.5.0 is live). |
| Windows installer (unsigned, early access) | After a Windows host runs the test suite on it and a person passes the Windows QA list. The build itself can be made in this container within days. |
| macOS .dmg | After a Mac host builds it and a person passes the macOS QA list. Notarized as soon as the Apple membership exists. An unsigned listing only with the owner's explicit OK and correct macOS 15 install steps. |
| Phone companion (PWA) | When the API, the portal and the relay are deployed and Pro can be bought (or the owner decides the companion is free during early access). |
| Android app (Google Play) | After the PWA path works, the native app passes on real phones, and the 14-day closed test is done. |
| iOS app (App Store) | After TestFlight on real iPhones and App Review approval. |

---

## 2. Facts from the repository

### 2.1 What ships, and how

- **The 0.5.0 release** is a Linux .deb only, `Vibezeo_0.5.0_amd64.deb`. It's built locally with `TANDEM_HOSTED=0 bash scripts/release-local.sh --dry-run`, unsigned, with no hosted service. It's served from `apps/web/public/downloads` with SHA256SUMS.
- **The download page's cards** come from `apps/web/lib/release.ts` (`DOWNLOADS`).
  - macOS and Windows already have cards with `available: false` ("Coming soon").
  - `app/site.test.tsx` asserts that only Linux is available.
  - This gate already exists. The plan keeps it and makes it stricter (stream R1).
- **The bundle identifier** in `apps/desktop/src-tauri/tauri.conf.json` is `com.vibezeo.tandem`. DECISIONS and ARCHITECTURE §17.1 still say `app.tandem.desktop`.
- **Bundle targets** are `appimage, deb, dmg, app, nsis`. `release.yml` adds `msi` on Windows.
- **The stack:** Tauri 2.12.1 (`@tauri-apps/cli` 2.12.1), wry 0.57, Rust 1.97 stable.

### 2.2 CI history: macOS and Windows have never run

- **The "CI" workflow (`ci.yml`) has never passed.** The repository has 151 workflow runs.
  - Since the evening of 7 October 2026 (UTC), every job fails within about 3 seconds with: "The job was not started because recent account payments have failed or your spending limit needs to be increased."
  - On the pushes to `main` (runs 37685855292, 37688765048, 37691546429, 38051397618), the jobs `desktop-build (macos-14)`, `test-rust (macos-14)`, `desktop-build (windows-2022)` and `test-rust (windows-2022)` each show **zero steps**.
- **No macOS or Windows job has ever executed a step.** The Release workflow (`release.yml`) has never run.
- **The only green runs** are from an older version of `ci.yml`: two Linux jobs (js and rust), before the desktop CI replaced it.
- **Pull requests run Linux only**, by design. macOS and Windows jobs run on pushes to `main` and on manual runs.
- **Minutes reset about 1 November** (HUMAN_TODO). The owner chose not to pay for more.

### 2.3 Platform code that already exists for macOS and Windows

| Area | macOS | Windows | Code | How it has been checked |
|---|---|---|---|---|
| Terminals | portable-pty, session leader, tree kill | ConPTY through portable-pty; Job Object with kill-on-close | `crates/tandem-pty/src/{lib,job,orphans}.rs` | Linux tests. The Job Object was type-checked with MinGW (BUILD_LOG M3). |
| Login environment | `$SHELL -lic` capture; GUI PATH fixes | Process env only; **registry merge missing** (§2.4) | `crates/tandem-pty/src/env.rs` | Linux tests |
| Shell integration | zsh, bash and fish shims | PowerShell shim (cmd has none) | `crates/tandem-pty/src/integration.rs` | Linux tests |
| Engine detection | Homebrew, nvm, `.zprofile` fix | PowerShell `Get-Command`, `%APPDATA%\npm` | `crates/tandem-engines/src/detect.rs` | Linux tests |
| Status bus | Unix socket | Loopback TCP with a Host check | `crates/tandem-engines/src/status_server.rs` | Linux tests |
| Dictate hotkeys | Listen-only `CGEventTap`, Input Monitoring | `WH_KEYBOARD_LL` that never consumes (guarded by `scripts/guards/no-consuming-hook.sh`) | `crates/tandem-voice/src/hotkey/{macos,windows}.rs` | Type-check and clippy from Linux (`scripts/agent/check-cross.sh`, with stub headers for macOS) |
| Dictate insertion | NSWorkspace plus AX focus, ⌘V | `SetForegroundWindow`, UIPI check, Ctrl+V | `crates/tandem-voice/src/insert/{macos,windows}.rs` | The same cross-check |
| Background routines | launchd LaunchAgent | `schtasks` every 5 minutes | `crates/tandem-routines/src/launcher.rs` | Linux tests with a fake `CommandRunner` |
| Browser panes | wry child webview (WKWebView) | wry child webview (WebView2) | `apps/desktop/src-tauri/src/browser/wry_host.rs` | Never compiled for either |
| WSL workspaces | n/a | Distros, folders, panes, threads, detection | `services/wsl.rs`, `tandem_core::wslpath` | Linux tests (parsing and path translation) |
| Window chrome | Overlay title bar, traffic lights | Custom controls (snap layouts in manual QA) | `tauri.macos.conf.json`, `features/shell/WindowControls.tsx` | Never run |
| Notifications | Plugin plus dock bounce | Plugin; the AUMID comes from the installer's shortcut | `services/notifier.rs` | Never run |
| Deep links | Info.plist through Tauri | Registered at runtime | `src-tauri/src/lib.rs` | Never run |
| Updates | `.app.tar.gz` | NSIS | `services/updates.rs`, `release.yml` | Never run; there's no updater key yet |
| SSH helper bundle | `tandem-host` macOS universal (lipo) | n/a (hosts are Linux or macOS) | `scripts/ci/build-tandem-host.sh` | macOS binary never built |

### 2.4 Gaps found in this review

1. **The macOS minimum version is unset.**
   - `tauri.conf.json` has no `bundle.macOS.minimumSystemVersion`, so Tauri's default of 10.13 applies.
   - DECISIONS and PRODUCT_SPEC §5.6 say macOS 14+, and the site's card says "macOS 10.15 or later".
   - Set 14.0 and fix the site.
2. **Dictate can't use the microphone on macOS.** There's no `Info.plist` with `NSMicrophoneUsageDescription` and no entitlements file. Tauri turns on the hardened runtime by default (`hardenedRuntime: true`). Under it:
   - the microphone needs the `com.apple.security.device.audio-input` entitlement, or TCC silently blocks it;
   - a missing usage string is fatal at the first microphone access.
3. **Notarization will reject an unsigned binary inside the bundle.**
   - Every Mach-O in the bundle must be signed with the Developer ID, the hardened runtime and a secure timestamp.
   - `host-binaries/macos-universal/tandem-host` ships as a resource, and Tauri signs the app's executables, not its resources. Sign it before bundling. HUMAN_TODO already asks for it to be codesigned. The first notarization run will confirm.
4. **`tandem-hook` isn't bundled on any OS.**
   - `tauri.sidecars.conf.json` lists only `tandem-tasks`.
   - D-68 says the hook must sit next to the app and that bundling it belongs to the release pipeline.
   - Without it, release builds get pane status from heuristics only. Add it to `externalBin` and `scripts/build-sidecars.sh`, and sign it on macOS and Windows.
5. **The Windows registry PATH merge isn't implemented.**
   - ARCHITECTURE §5.2 asks for it. `run_capture` in `crates/tandem-pty/src/env.rs` returns the process env, and the repository has no registry code.
   - An app started from Explorer misses engines installed after login.
6. **SSH workspaces won't work on Windows as built.**
   - `crates/tandem-host/src/ssh.rs` starts a ControlMaster (`ssh -M -N -f`, D-404) and joins it with `ControlPath`.
   - Microsoft's Win32-OpenSSH client doesn't implement connection multiplexing (§3.2). The options are accepted, but no master is created.
   - Windows needs its own path: either a plain connection per call with key auth through the Windows ssh-agent, or an in-process SSH client (§4.6). PRODUCT_SPEC F-RM-01 says "Windows: OpenSSH client" without noting this.
7. **The Job Object is assigned after the process starts.**
   - `Job::for_process(pid)` runs after `spawn_command`. ARCHITECTURE §5.3 says "created before resume".
   - A child started in that first instant could escape the job. The HUMAN_TODO M3 check (closing a pane ends its whole tree) must cover it.
8. **The Windows signing tool may have a new name.** `release.yml` calls `trusted-signing-cli`. Microsoft renamed the service "Artifact Signing", and Tauri's docs now use `artifact-signing-cli`. Recheck the tool name and flags when the secrets exist.
9. **The site lists an MSI that a Linux cross-build can't make.** Only Windows can build MSI (WiX). A cross-build produces NSIS only. Drop the `.msi` asset from the site card unless it's built on Windows.
10. **The site's macOS install steps are out of date.** They say right-click › Open. macOS 15 removed that bypass: users go to System Settings › Privacy & Security › Open Anyway. Rewrite the steps before any unsigned macOS listing.
11. **The identifier docs don't match the config.** Keep `com.vibezeo.tandem`: store and bundle IDs are permanent once published. Fix DECISIONS, ARCHITECTURE §17.1 and the AUMID text.
12. **Decision numbers collide.** `product-m0-m5` already uses D-1000 to D-1008 (the voice API, F-BE-08), and M16C uses D-1000 to D-1017. Renumber M16C's decisions when it merges.
13. **The `m11-windows-ready` branch can't be found.** BUILD_LOG lists it ("Windows readiness checked from Linux") as in flight, but there's no such branch on `origin`. Ask its session for its state before stream W1 starts, so the work isn't done twice.

### 2.5 The phone companion (M16C, unmerged)

- **Branch:** `origin/m16c-phone-companion`, head 37c1561, 10 October 2026. It changes 105 files (+9,139/−27) and isn't merged.
- **F-BE-11 relay** (`apps/api/src/routes/relay.ts`):
  - A WebSocket relay in the API process, `GET /v1/relay/:pairing_id`.
  - The desktop authenticates with its bearer token; the phone uses the subprotocol `tandem-pair.<token>`.
  - It forwards opaque binary frames of up to 64 KB, at 50 frames a second per pairing, and holds no crypto imports (ESLint rule plus a test).
  - Pairings are created at `POST /v1/pairings`, which needs Pro: `limits.phone` and `assertPhoneAllowed`.
- **F-RM-03 protocol:**
  - `tandem_core::remote::{protocol,messages}` in Rust and `apps/web/lib/remote/protocol.ts` in TypeScript.
  - Both check every byte against `crates/tandem-core/tests/fixtures/remote_v1.json`.
  - Pairing is by QR with a HMAC tag, then X25519 and HKDF-SHA256, then XChaCha20-Poly1305 frames with monotonic counters and a desktop-led rekey.
- **The phone's commands:** six (`view.snapshot`, `thread.read`, `thread.send`, `pane.type`, `approval.decide`, `routine.run_now`) under four grants (chat, terminal, approvals, routines), plus view.
- **Desktop side:** `services/phone.rs` (PhoneService), ten commands and Settings › Remote.
- **PWA:** `apps/web/app/remote` (RemoteApp, manifest, service worker), `lib/remote/client.ts` and an IndexedDB store sealed with a non-extractable AES-GCM key.
- **Tests:** E2E-M16-4 (5 tests: pairing over the real relay, replay rejected, an allow queued until it's armed, the 65 KB close, revoke), web @m16 (2 tests), 33 Rust tests and about 25 TypeScript tests.
- **What limits it as a phone product today:**
  - It polls every 10 s while visible (`POLL_MS`) and has no push notifications.
  - Pairing goes through the phone's camera app and opens a web page.
  - Keys sit in browser storage. D-1009 documents the residual risk.
  - Pairing needs a signed-in Pro desktop and the hosted relay, so early-access builds (`TANDEM_HOSTED=0`) can't pair at all. `api.vibezeo.com` and `app.vibezeo.com` aren't deployed (HUMAN_TODO).
- **The spec rules out native apps and stores.** PRODUCT_SPEC §6 lists as out of scope: "3. Native iOS or Android apps. The phone companion is a PWA (F-RM-03)." and "8. An App Store / Microsoft Store distribution." The owner has to change both (§6).

### 2.6 Remote workspaces (tandem-host) and phones

- **`tandem-host`** runs only on Linux and macOS hosts (`main.rs` is `cfg(unix)`).
- **The desktop's transport is the system `ssh`** with a ControlMaster.
- **The client doesn't care about the transport.** `tandem_host::client::HostClient::start(reader, writer)` takes any `AsyncRead`/`AsyncWrite`.
- **A phone would need an in-process SSH client.** It has no `ssh` binary, and iOS can't spawn processes, but a pure-Rust client such as russh can feed `HostClient` directly (§4.6). The same transport would also fix gap 6 on Windows.

### 2.7 This container, as a build machine

- **Machine:** Linux x86_64, 4 vCPU, 15 GB RAM, about **8 GB of free disk**. It's a Firecracker VM with **no `/dev/kvm`**, so no Android emulator.
- **Present:**
  - Java 21 (OpenJDK) and Gradle 8.14.3.
  - clang 18, `clang-cl-18`, `lld-link`, `llvm-lib`, `llvm-rc`, cmake and ninja: the cross tools cargo-xwin needs.
  - cargo-zigbuild, gcloud (preconfigured for the proxy) and Rust targets for linux-gnu and linux-musl.
- **Missing:**
  - The Android SDK and NDK, NSIS, Wine and cargo-xwin.
  - MinGW too: `scripts/bootstrap-linux.sh` installs it.
- **Network, through the agent proxy:**
  - These answer: `dl.google.com` (the Android SDK), `maven.google.com`, `services.gradle.org` and Maven Central.
  - JVM tools are already pointed at the proxy and its CA (`JAVA_TOOL_OPTIONS`).
  - **github.com downloads are allowed only for repositories attached to the session.** Anything else answers 403 "GitHub access to this repository is not enabled for this session". Tauri's NSIS bundler downloads `nsis_tauri_utils.dll` from `github.com/tauri-apps/nsis-tauri-utils`, so attach that repository with `add_repo` (read access) or pre-seed Tauri's tool cache. `raw.githubusercontent.com` works.

---

## 3. What the platforms require (external research)

### 3.1 macOS

- **Bundles and minimums.** Tauri builds `.app` and `.dmg` with `--target universal-apple-darwin`. Without a setting, `bundle.macOS.minimumSystemVersion` is 10.13.
- **Signing.**
  - Distribution outside the App Store needs a **Developer ID Application** certificate. Only the Account Holder of a paid Apple Developer Program membership ($99 a year) can create one.
  - Notarization is required with that certificate.
  - Credentials for notarization are either an App Store Connect API key (`APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH`) or an Apple ID with an app-specific password (`APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`).
  - Free accounts can't notarize.
- **Unsigned builds.**
  - Ad-hoc signing (`"signingIdentity": "-"`, which `release.yml` already uses without secrets) lets Apple-silicon Macs run the app. Users must still allow it in Privacy & Security.
  - In macOS 15 Apple removed the Control-click › Open bypass. Users click Open Anyway in System Settings › Privacy & Security and enter an admin password.
- **Signing from Linux.** `rcodesign` (apple-codesign) signs and notarizes from Linux with an App Store Connect API key. Building still needs the macOS SDK, whose licence ties it to Apple hardware, so the build itself needs a Mac. osxcross isn't recommended.
- **Hardened runtime.** It's on by default. The microphone needs `com.apple.security.device.audio-input` plus `NSMicrophoneUsageDescription`, or TCC blocks it silently.
- **Desktop E2E.** macOS has no WKWebView driver for `tauri-driver`. `@wdio/tauri-service` supports macOS through an embedded WebDriver server, the `tauri-plugin-wdio-webdriver` plugin, compiled into a test build.
- **Mac App Store:** not suitable. It requires the App Sandbox, and the desktop app spawns shells and CLIs.

### 3.2 Windows

- **Installer formats.**
  - **NSIS** (`-setup.exe`) can be cross-built from Linux.
  - **MSI** needs WiX, which runs only on Windows.
  - The default NSIS install is per user, to `%LOCALAPPDATA%`, with no administrator rights. That matches ARCHITECTURE §17.1.
- **Cross-building from Linux** (Tauri docs):
  - Install the tools: `sudo apt install nsis lld llvm` (plus `clang` for C/C++ dependencies), `rustup target add x86_64-pc-windows-msvc`, `cargo install --locked cargo-xwin`.
  - Build: `pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc`. The output is in `target/x86_64-pc-windows-msvc/release/bundle/nsis/`.
  - Tauri calls this "not as straight forward" and "not tested as much", and recommends it only as a last resort.
  - Only the MSVC target is supported, and signing needs a custom `signCommand`.
- **cargo-xwin** downloads the MSVC CRT and the Windows SDK. Its README states that using it is consent to Microsoft's licence, so **the owner must accept that licence**. CMake-built dependencies such as whisper.cpp need `ninja` (present here). `cargo xwin test` can run tests under Wine.
- **WebView2.** The default install mode is `downloadBootstrapper` (0 MB added, needs the internet). `offlineInstaller` adds about 127 MB. Windows 10 (1803 and later) and 11 ship the runtime.
- **Signing.**
  - Since 2024 an EV certificate no longer gives instant SmartScreen reputation. OV and EV build reputation the same way.
  - **Artifact Signing** (formerly Trusted Signing) costs $9.99 a month on the Basic plan (5,000 signatures).
  - Its Public Trust certificates are available to **organizations** in the US, Canada, the EU, the UK, Australia, New Zealand, Japan, South Korea, Singapore, Switzerland, Norway and Israel. **Individual developers must be in the US or Canada.**
  - Identity validation takes 1 to 20 business days.
  - jsign 7.0 and later can sign with it from Linux (Java). The fallback is an OV certificate on a cloud HSM.
  - Unsigned builds work, but users see SmartScreen ("More info › Run anyway"). Where Windows 11's Smart App Control is on, it blocks unknown unsigned apps with no per-app override.
- **Microsoft Store.** It has been free for individual developers since September 2025, and it signs MSIX packages. PRODUCT_SPEC §6 item 8 excludes store distribution, so it's listed here only as an option.
- **SSH.** Win32-OpenSSH accepts `ControlMaster` options but creates no master (gap 6).
- **Desktop E2E.** `tauri-driver` works on Windows, so the existing WebdriverIO suite can run there.

### 3.3 Android (Tauri 2 and Google Play)

- **Toolchain** (Tauri prerequisites):
  - From the SDK Manager: SDK Platform, Platform-Tools, NDK (side by side), Build-Tools and Command-line Tools.
  - Environment: `JAVA_HOME`, `ANDROID_HOME`, `NDK_HOME`.
  - Rust targets: `rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android`.
  - Android builds on Linux.
- **Build.**
  - `pnpm tauri android build --aab` for Play, or `--apk` with optional `--split-per-abi` and `--target aarch64`.
  - The `versionCode` comes from the version (`major*1000000 + minor*1000 + patch`).
  - Tauri's default `minSdkVersion` is 24. The current template targets the latest SDK (on Tauri's dev branch: compileSdk and targetSdk 37, AGP 9.3.1, Gradle 9.6.1).
  - The first Play upload must be made by hand.
  - Signing uses an upload keystore plus `keystore.properties` and an edit to `build.gradle.kts`. Play App Signing holds the app signing key.
- **Google Play rules in force now:**
  - **Target API.** Since 31 August 2026, new apps and updates must target **API 36** (Android 16). An extension to 1 November 2026 can be requested.
  - **16 KB pages.** Since 1 November 2025, apps targeting Android 15+ must support 16 KB page sizes on 64-bit devices. NDK r28 and later align for that by default. Check with `zipalign -c -P 16 -v 4`.
  - **Closed test.** Personal accounts created after 13 November 2023 must run a closed test with **at least 12 testers opted in continuously for at least 14 days** before they can apply for production. Organization accounts (with a D-U-N-S number) are exempt.
  - **Developer verification.** It went live on **30 September 2026** for users in Brazil, Indonesia, Singapore and Thailand, and goes global for certified devices **in 2027**. Play developers are registered automatically. Distributing an APK outside Play needs the Android Developer Console. Unverified apps fall to an "advanced flow" for sideloading.
- **Keychain on Android.**
  - `keyring` v3 (in the workspace now) has no Android store. It falls back to its mock store, which would keep secrets in memory only: silently wrong.
  - The `keyring-core` 1.x family has **`android-native-keyring-store` 1.0.0** (April 2026). It stores secrets in SharedPreferences, encrypted with Android Keystore keys, and Tauri Mobile already initializes the `ndk-context` it needs.
  - `apple-native-keyring-store` covers iOS.
- **Testing without an emulator.** Firebase Test Lab's free Spark plan runs 10 virtual-device and 5 physical-device test runs a day (Robo, instrumentation). It can be driven from this container with `gcloud`.

### 3.4 iOS (Tauri 2 and the App Store)

- **Toolchain.** Needs macOS with full Xcode (Command Line Tools aren't enough), Homebrew and CocoaPods, plus the targets `aarch64-apple-ios`, `x86_64-apple-ios` and `aarch64-apple-ios-sim`.
- **Build and upload.**
  - Build: `pnpm tauri ios build --export-method app-store-connect`.
  - Upload: `xcrun altool --upload-app --type ios --file … --apiKey $APPLE_API_KEY_ID --apiIssuer $APPLE_API_ISSUER`.
  - Signing is automatic with an App Store Connect API key (`APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH`), or manual (`IOS_CERTIFICATE`, `IOS_CERTIFICATE_PASSWORD`, `IOS_MOBILE_PROVISION`).
  - `bundle.iOS.developmentTeam` (or `APPLE_DEVELOPMENT_TEAM`) is required.
  - The default minimum iOS version is 15.0.
- **Apple rules in force now:**
  - Since **28 April 2026**, uploads must be built with **Xcode 26 and the iOS 26 SDK**. Xcode 26 needs macOS 15.6 or later.
  - Since May 2024, uploads that use "required reason" APIs without declaring them in a privacy manifest are rejected (ITMS-91053).
  - Encryption needs an export-compliance answer (`ITSAppUsesNonExemptEncryption`).
  - EU storefronts require verified trader status (Digital Services Act).
- **App Review.**
  - **2.1:** a login needs a demo account (with the back end on). A built-in demo mode is allowed only "with prior approval by Apple".
  - **3.1.3(f):** "Free apps acting as a stand-alone companion to a paid web based tool … do not need to use in-app purchase, provided there is no purchasing inside the app, or calls to action for purchase outside of the app." That fits the companion.
  - **4.2:** the app must be more than "a repackaged website".
  - **4.2.3(i):** "should work on its own". A demo mode helps here.
  - **4.2.7:** a remote-desktop app that mirrors specific software must be LAN-only. That matters if the phone ever streams the desktop's own terminals (§4.6).
- **TestFlight:** internal testing (team members, no review) and external testing (Beta App Review).
- **Keychain:** `keyring` v3's `apple-native` feature already supports the iOS Keychain.

### 3.5 Tauri 2 on mobile: plugins and gaps

| Need | Plugin | Mobile support (official table) |
|---|---|---|
| Scan the pairing QR | barcode-scanner | iOS and Android |
| Open the app from the pairing link | deep-link (app links, universal links) | All platforms |
| Local notifications | notification | All platforms, **local only** |
| Remote push (APNs, FCM) | **None official.** Community plugins: `tauri-plugin-mobile-push`, Choochmeque's `tauri-plugin-notifications`. Or our own plugin (Kotlin/Swift). | n/a |
| Confirm on the device | biometric | iOS and Android |
| Key-value settings | store | All platforms |
| HTTP | http | All platforms (the Rust side can also use `reqwest` and `tokio-tungstenite` directly) |
| Not available on mobile | updater (stores update apps), single-instance, window-state, autostart, positioner, cli | n/a |

- **WebViews.** Android uses the Android System WebView (Chromium, updated through Play). iOS uses WKWebView.
- **Web Push for the PWA.** iOS and iPadOS 16.4+ allow Web Push for web apps added to the Home Screen, with permission requested from a tap. Android Chrome supports it too.

### 3.6 Build hosts without paid Actions minutes

| Option | macOS | Windows | Cost | Notes |
|---|---|---|---|---|
| GitHub-hosted runners, **public** repository | Yes | Yes | Free | Standard hosted and self-hosted runners stay free on public repositories. Making the repository public publishes the product's code (no LICENSE file today; the Cargo metadata says MIT). Business decision. |
| GitHub-hosted runners, private repository | Yes | Yes | 2,000 included minutes a month (Free plan; 3,000 on Pro), reset monthly | 2026 rates: macOS 3/4-core $0.062/min, Windows $0.010/min, Linux $0.006/min. macOS has historically used included minutes about 10 times faster than Linux. |
| **Self-hosted runner** on a Mac or Windows PC the owner has | Yes | Yes | Free | GitHub postponed its planned $0.002/min self-hosted charge with no new date. Probe once whether this account's billing block also stops self-hosted jobs. |
| **Azure Pipelines free tier** (private project) | Yes | Yes | 1 Microsoft-hosted job, up to 60 minutes per run, **1,800 minutes a month** | Turned on by linking an Azure subscription. Public projects are retired. A universal macOS build should be split into one job per architecture to stay under 60 minutes. |
| **Codemagic** (individual plan) | M2 | No | **500 macOS minutes a month**, builds up to 120 min, 1 at a time | Good for iOS and macOS. |
| **Xcode Cloud** | Xcode projects | No | **25 compute hours a month** with the Apple membership | Fits the iOS app (Tauri generates an Xcode project). Rust and Node install in a custom script. |
| CircleCI free plan | Yes | Yes | 30,000 credits a month (third-party figures: about 150 macOS or 750 Windows minutes) | Check current rates. |
| Cirrus CI | No | No | Shut down 1 June 2026 | Don't use. |
| Firebase Test Lab (Spark) | No | No | 10 virtual and 5 physical Android test runs a day | Android only. Driven from this container with gcloud. |

**What this means for Vibezeo:**

1. **From now on, Linux builds, the gate and E2E stay in agent containers**, as they already do. Nothing new is needed.
2. **The cheapest zero-cost path for macOS and Windows** is the Azure Pipelines free tier. A self-hosted runner on any Mac or Windows PC the owner has works too. Use Codemagic or Xcode Cloud for iOS.
3. **After 1 November, keep GitHub's included minutes for macOS and Windows runs, started by hand.** Stop spending minutes on every pull request. One universal macOS build of about 30 to 40 minutes uses roughly 300 to 400 included minutes at the historical rate, so expect 3 to 5 macOS-plus-Windows runs a month. Measure the first run.
4. **If the owner later accepts a small budget,** a macOS build costs about $2 at $0.062/min.

---

## 4. What "the same product" means on each platform

### 4.1 macOS and Windows: the full desktop app

**macOS and Windows get everything the Linux build has:**
- terminals and agent panes, Thread mode, the Dashboard, Agent mode and teammates;
- Dictate, tasks and routines, plugins and approvals;
- SSH workspaces, Files, git and the browser panes, the Scratchpad, Find work and data controls.

**Platform extras:**
- **macOS:** the overlay title bar, Option-as-Meta, launchd routines and, once `m16d-simulator` merges, the Simulator pane (F-CD-21).
- **Windows:** WSL workspaces, PowerShell integration and Task Scheduler routines.

**Distribution:**
- The same early-access mode first (`TANDEM_HOSTED=0`: the Free plan locally, no account).
- Direct downloads only. The app stores don't fit: the Mac App Store needs the sandbox, and PRODUCT_SPEC §6 item 8 excludes stores.

**Untested, and to be stated on the site:**
- Intel Macs, if only Apple silicon is tested.
- Windows on Arm, which runs x64 under emulation. Native ARM64 is out of scope (PRODUCT_SPEC §6 item 11).

### 4.2 Android and iOS: the companion, not the desktop

**Why not the desktop app:**
- **iOS.** App Store apps can't spawn processes, and guideline 2.5.2 forbids downloading or executing code. That rules out shells, PTYs and the coding CLIs.
- **Android.** The CLIs need Node or Python and a POSIX environment. Since API 29, apps can't execute files in their writable home directory, and Play forbids downloading executable code. Termux-style setups sit outside Play policy.
- **Either way, the work lives on the desktop.** The user's repositories, provider sign-ins and engines are there, and provider homes never leave the machine (CLAUDE.md).

**Vibezeo Remote, v1** (parity with the PWA, plus what only an app can do):

| In v1 | Detail |
|---|---|
| Pair | Scan the desktop's code in the app, or open the same link. |
| Agents | Status and the latest redacted activity line. |
| Approvals | Deny always applies. Allow needs the desktop's 8-hour arming, or it waits for local confirmation (F-TL-06 rule 5, unchanged). |
| Threads and teammate chats | Read, and send with the Chat grant. |
| Type to agent panes | With the Terminal grant; never plain shells. |
| Routines | Run now, with the Routines grant. |
| **Notifications** (new) | "An agent needs you" or "finished", opt-in, end-to-end encrypted (§4.5). |
| Unpair | From the phone. |
| Demo mode | For App Review and curious visitors. |

**Not in v1:** live terminal output, Files, git, Dictate on the phone, plugins, settings and billing. The phone keyboard's own dictation already works in the composer.

**Tablets:** the same app with a two-column layout. iPads and Android tablets can't run the CLIs either.

### 4.3 Mobile options weighed

| | A. Tauri mobile build of the desktop React UI | B. Wrap the PWA as-is (TWA, Capacitor or a WKWebView shell) | **C. Tauri 2 app: PWA screens plus the Rust protocol (recommended)** | D. PWA only |
|---|---|---|---|---|
| UI reuse | High in theory, useless in practice: the UI is a client of 336 Tauri commands (359 `invoke` call sites in 95 files) backed by PTYs, engines, git2, the gateway and whisper, none of which run on a phone | Full | Full (the PWA's screens move to a shared package) | Full |
| Phone layout | No: a desktop shell (minimum window 960×600), panes and an xterm WebGL pool | Yes | Yes | Yes |
| Security model | Breaks it: mapping desktop IPC over the relay grows the phone's command set from 6 to hundreds, against F-TL-06 rule 5, the 64 KB frame cap and 50 frames/s | Keys stay in web storage (D-1009) | Keys in the Keychain or Keystore; crypto in Rust, already vector-tested | Keys in web storage |
| Fits DECISIONS (Tauri 2, Rust plus React) | Yes | No: a second toolchain | Yes | Yes |
| Push notifications | Would need native code | Needs native code in the shell | Our own small plugin plus APNs/FCM | Web Push only (iOS 16.4+ Home Screen apps) |
| QR and deep links | Needs work | Partial | barcode-scanner and deep-link plugins | Camera app only |
| App Review risk | High | High (4.2 "repackaged website") | Low to medium (native features, demo mode) | n/a (no store) |
| Effort | Very high | Low | Medium | None (built) |

**Recommendation: D now, C next.**
- The PWA is the zero-install channel and the fallback. It stays.
- The native apps use C.
- A is rejected outright. B adds a toolchain and keeps the key-storage risk without removing native work.

### 4.4 Recommended mobile architecture (option C)

**Layout.** This needs the owner's approval because DECISIONS fixes the layout:

- `apps/mobile/`: Vite, React 19, Tailwind 4 and `@tandem/ui`, the same front-end stack as the desktop.
- `apps/mobile/src-tauri/`: crate `tandem-mobile` (lib `tandem_mobile`). Tauri 2 mobile targets only.
- `packages/remote/`: shared TypeScript, used by the PWA and the app:
  - a `CompanionBackend` interface (`pair`, `resume`, `command`, `unpair`, plus events);
  - the screens (Agents, Approvals, Threads, Routines, Type, pairing help);
  - the web backend: today's `PhoneClient` with `IndexedDbPairingStore`.

**Rust core:**
- **The phone side of the protocol** goes in `tandem_core::remote::phone`. It ports `apps/web/lib/remote/client.ts`:
  - the hello with its HMAC tag, then `hi` and `welcome`;
  - sealed commands with ids and timeouts;
  - `rekey` and `rekey_ack`;
  - the send counter persisted before each frame;
  - a wipe on `bye` or a 1008 close.
  - The relay socket uses `tokio-tungstenite` with rustls and the `tandem-pair.<token>` subprotocol.
  - Tests check it against `remote_v1.json` and a fake relay.
- **Keep the mobile build small.** Put `tandem-core`'s desktop-only dependencies (`notify`, `ignore`, file watching) behind a feature, or move the remote protocol into a small crate. Either way it's a layout decision.
- **Keys** use the existing `Keychain` trait, implemented with `keyring-core` plus `apple-native-keyring-store` (iOS) and `android-native-keyring-store` (Android). Never IndexedDB. This stays within CLAUDE.md's "keyring crate" rule. Record the move to `keyring-core` as a decision.
- **IPC, declared in ARCHITECTURE §3 first:**
  - commands: `companion_status`, `companion_pair{link}`, `companion_resume`, `companion_command{cmd,args}`, `companion_unpair`, `companion_notify_set{on}`;
  - events: `companion:state`, `companion:changed`, `companion:rejected`.

**Pairing link:**
- Keep today's link: `https://app.vibezeo.com/remote#p=…`.
- With Universal Links (`apple-app-site-association`) and Android App Links (`assetlinks.json`) on `app.vibezeo.com`, the phone's camera opens the app when it's installed and the PWA otherwise. The app also has its own scanner.
- The fragment never reaches a server or a log.

**On the device:**
- An optional Face ID, fingerprint or passcode check before an allow or before typing into a pane (biometric plugin).
- That check is in addition to the desktop's arming, never instead of it.

**Proposed identity:**
- Bundle and package `com.vibezeo.remote`, name "Vibezeo Remote", after the trademark check.
- Android `minSdk` 26 and `targetSdk` 36 or later.
- iOS: Tauri's default 15.0 unless a dependency needs more.

### 4.5 Notifications

**Native apps (new spec items F-RM-04 "Phone notifications" and F-BE-13 "Push relay"):**

1. The phone gets an APNs or FCM token. It sends the token to the desktop inside the encrypted channel and registers it with the API: `PUT /v1/pairings/:id/push`, authenticated with the pairing token.
2. When an approval waits, an agent waits or finishes, or a routine ends, the desktop seals a small notice (`{kind, title, ref}`; message text only if "Include message preview" is on, as for desktop notifications). The key is `k_push = HKDF(root, "tandem-push-v1"‖pid)`.
3. The desktop posts it: `POST /v1/pairings/:id/notify {ct}`.
4. The API forwards opaque bytes:
   - to APNs, with `mutable-content: 1` and a generic fallback alert;
   - to FCM, as a data message.
   - It stores nothing, and the route has no crypto imports, enforced by the same ESLint rule as the relay.
5. An iOS Notification Service Extension, or an Android `FirebaseMessagingService`, decrypts with the key from the keychain (an app group or shared access group on iOS) and shows the real text. If decryption fails, it shows the generic text.

**Rules:**
- **Privacy:** Apple, Google and our API see the timing, the size and a device token, never the content.
- **Opt-in:** per phone, off by default (DECISIONS rule 4).
- **Subprocessors:** APNs and FCM are added to `packages/protocol/subprocessors.json`.
- **Tests:** `MockPush` providers mean tests never need real keys (DECISIONS rule 3).

**PWA (optional):** Web Push with the desktop as the application server. The desktop holds its own VAPID key and encrypts each payload per RFC 8291, so the server holds no secret.

### 4.6 Later, optional: terminals on SSH hosts from the phone

- **What:** open a terminal on one of the user's own SSH hosts from the phone, through `tandem-host`.
- **How:**
  - russh (pure Rust SSH) runs in the app's Rust core and opens an exec channel `~/.tandem-host/<proto>/tandem-host serve`.
  - `HostClient::start(reader, writer)` drives it, using `pty.spawn` and `pty.attach`.
  - The helper is uploaded from bundled binaries checked against the manifest's SHA-256, as the desktop does.
  - xterm.js in the app gets an extra key row (Esc, Ctrl, Tab, arrows).
- **Trust:**
  - The phone generates an Ed25519 key and keeps it in the keychain.
  - "Send host to phone" (confirmed on the desktop) passes the host entry and its pinned host key over the encrypted channel.
  - The user adds the phone's public key to `authorized_keys`, or the desktop does it after a local confirmation.
- **Reach:** the phone must reach the host directly (a public host or a VPN such as Tailscale).
- **Tests:** F-RM-01's acceptance tests run on Linux against the existing Docker `sshd` fixture, with the russh transport.
- **Shared with Windows:** the same transport fixes gap 6 (no ControlMaster on Windows).
- **Risks:**
  - xterm.js with mobile keyboards (Android composition) is rough.
  - Apple accepts generic SSH clients. Streaming the desktop's own panes through the relay instead would risk guideline 4.2.7 (LAN-only for mirrors of specific software).

### 4.7 Later, optional: a LAN relay for builds without accounts

- **What:** the desktop serves the same dumb relay on the local network. It uses TLS with a self-signed certificate whose fingerprint is in the QR. The protocol inside doesn't change.
- **Limits:**
  - Native app only: browsers block a page served over HTTPS from opening a plain-WebSocket connection, and they restrict pages from reaching addresses on the local network.
  - It works without an account, so in early-access builds.
  - No push. iOS asks for Local Network permission.
- **Decision needed:** it changes F-BE-11 ("the relay runs in the API") and the Pro gating. It isn't in the recommended v1.

---

## 5. Plan per platform

### 5.1 macOS

| | |
|---|---|
| **Scope** | Universal `.app` and `.dmg`, macOS 14+. Developer ID signed, notarized and stapled. Updater `.app.tar.gz` plus `.sig` once the minisign key exists. Sidecars (`tandem-tasks`, `tandem-hook`) and the macOS `tandem-host` signed. |
| **Architecture changes** | Add `bundle.macOS.minimumSystemVersion: "14.0"` and an `Info.plist` (`NSMicrophoneUsageDescription`). Add an `Entitlements.plist` (`com.apple.security.device.audio-input`; add `com.apple.security.cs.disable-library-validation` only if ONNX Runtime loads a dylib signed by another team). Sign `tandem-host` before bundling. No sandbox. |
| **In this container** | The config, plist and entitlements, with a test that pins them. Extend `check-cross.sh` from `tandem-voice` to the other crates with macOS code (crates whose C builds need the SDK will fail; list them). The release script and `release.yml` changes. Site copy (macOS 14+, macOS 15 install steps). Optional: signing and notarization on Linux with rcodesign. |
| **Needs a Mac (CI or the owner's Mac)** | Compile and `cargo test --workspace`. `tauri build --target universal-apple-darwin --bundles app,dmg`. A smoke test (launch, window, screenshot). An E2E subset with `@wdio/tauri-service` and its embedded driver, in a test-only feature that `scripts/check-release-harness.sh` proves absent from release builds. Notarization. |
| **Manual QA (HUMAN_TODO, macOS items)** | Dictate permissions (Input Monitoring, Accessibility, Quit & Reopen), Parakeet latency on an M1, IME and Option-as-Meta, the overlay title bar, the `tandem://` scheme, launchd routines, dock bounce, browser panes on WKWebView, orphans after `kill -9`, GUI PATH detection, a real signed update. |
| **Gate before vibezeo.com lists it** | 1. The `.dmg` was built by CI from the release tag. 2. It's in SHA256SUMS. 3. `spctl -a -vv` accepts it and `xcrun stapler validate` passes, or the owner approves an unsigned listing with the macOS 15 steps. 4. The smoke test and the QA list are recorded in BUILD_LOG (date, Mac model, macOS version, tester). 5. CHANGELOG `[mac]` bullets cover only what was tested. |

### 5.2 Windows

| | |
|---|---|
| **Scope** | Per-user NSIS installer (x64) with the WebView2 download bootstrapper. MSI only if built on Windows. Authenticode signing once possible. `tandem-tasks.exe` and `tandem-hook.exe` sidecars. The AUMID on the Start Menu shortcut. The uninstaller removes the `schtasks` entry (ARCHITECTURE §10.3). |
| **Code changes** | The registry PATH merge (HKCU and HKLM `Environment`), behind a trait with a fake. The Job Object assigned before the child can run, or proven safe. An SSH path without ControlMaster (gap 6). |
| **In this container** | Whole-workspace `cargo xwin clippy` for `x86_64-pc-windows-msvc` once the owner accepts Microsoft's licence. Without that, type-check the GNU target with MinGW, as before. Fix compile errors. Unit tests with fakes. The NSIS cross-build (`apt install nsis`, the `clang-cl` link, `add_repo tauri-apps/nsis-tauri-utils`). A bundle check (`7z l`). Signing with jsign as the `signCommand` once secrets exist. Optional unit tests under Wine. |
| **Needs Windows (CI or the owner's PC)** | `cargo test --workspace` on Windows Server 2022 or 2025. `tauri build --bundles nsis,msi`. E2E through `tauri-driver` with a matching `msedgedriver`, skipping the specs that depend on Linux tools. Windows-only tests: Job Object tree kill, ConPTY resize, registry PATH, a `schtasks` round trip in a temporary folder. |
| **Manual QA (HUMAN_TODO, Windows items)** | ConPTY TUIs (vim, htop), closing a pane kills its tree, PowerShell 5.1 and 7 integration, WSL (including a dotted distro name, WSL 1, non-English Windows), Japanese IME, snap layouts, toasts and their AUMID, Task Scheduler routines, the Dictate hook and the Start menu, an elevated target window, SSH to a real host, SmartScreen behaviour. |
| **Gate** | As for macOS. Signed (`Get-AuthenticodeSignature` Valid, with a timestamp), or the owner approves an unsigned listing with SmartScreen steps and the Smart App Control caveat. |

### 5.3 Android

| | |
|---|---|
| **Scope** | Vibezeo Remote. An AAB for Play and APKs for testing. minSdk 26, targetSdk 36+, 16 KB aligned. `arm64-v8a`, plus `armeabi-v7a` and `x86_64` for older phones and Chromebooks. |
| **In this container** | **Build:** install the command-line tools, platform 36, build-tools and NDK r28+, plus the four Rust targets, then `pnpm --filter mobile tauri android build --apk --target aarch64`. **Check:** Rust unit tests; vitest for the screens; Playwright with phone emulation against the Vite dev server; `zipalign -c -P 16 -v 4`; `llvm-readelf -l` (LOAD align 0x4000); `apksigner verify`. **Device runs:** Firebase Test Lab through gcloud (a Robo run that reaches the pairing screen). **Disk:** about 5 GB for the toolchain, against 8 GB free here, so prune `target/` first or use a container per stream. |
| **Needs people** | A real phone to test pairing and push. A Play Console account. 12 testers for 14 days. |
| **Gate** | A Google Play badge on the site only when the production listing is live. A testing link only labelled "beta". No APK on the site unless developer verification is done and the APK is tested. |

### 5.4 iOS

| | |
|---|---|
| **Scope** | Vibezeo Remote for iPhone (iPad with the same UI), iOS 15+. |
| **In this container** | The shared UI and the Rust protocol tests only. |
| **Needs a Mac with Xcode 26 (macOS 15.6+)** | `tauri ios init` (commit `gen/apple`), simulator runs, device builds, signing, the IPA, the TestFlight upload (altool with an API key), the Notification Service Extension target, the privacy manifest and the export-compliance key. CI: Codemagic (500 min), Xcode Cloud (25 h), GitHub macOS runners or the owner's Mac. |
| **App Review preparation** | 1. A demo account with an always-on demo desktop, or Apple's prior approval for the built-in demo mode. 2. Reviewer notes. 3. No purchase or price talk in the app (3.1.3(f)). 4. App Privacy answers, DSA trader information and an age rating. |
| **Gate** | An App Store badge only at "Ready for Distribution". A TestFlight public link only labelled "beta". |

### 5.5 What runs where

| Work | This container | A Mac | A Windows PC | Free CI options |
|---|---|---|---|---|
| Linux build, gate, E2E | **Yes** (as today) | n/a | n/a | n/a |
| Windows type-check and clippy | **Yes** (cargo-xwin, or MinGW for the GNU target) | n/a | Yes | Azure Pipelines, self-hosted runner |
| Windows NSIS installer | **Yes** (cargo-xwin, unsigned or jsign-signed) | n/a | Yes | Same |
| Windows MSI | No | No | Yes | Same |
| Windows tests and E2E | Partial (Wine for logic only) | No | **Yes** | Azure Pipelines, GitHub after 1 Nov, self-hosted runner |
| macOS type-check | Partial (stub headers; crates with C dependencies fail) | Yes | No | n/a |
| macOS build and tests | No (SDK licence) | **Yes** | No | Azure Pipelines (two arch jobs), Codemagic, GitHub after 1 Nov, self-hosted runner |
| macOS signing and notarization | Possible with rcodesign | Yes | No | n/a |
| Android APK/AAB | **Yes** (SDK, NDK, Java 21 present) | Yes | Yes | n/a |
| Android device tests | Via Firebase Test Lab (gcloud) | Emulator | Emulator | Test Lab Spark |
| iOS build and tests | No | **Yes** | No | Codemagic, Xcode Cloud, GitHub macOS runners |

---

## 6. Decisions the owner must make first

| # | Decision | Recommendation | Why it blocks |
|---|---|---|---|
| OD-1 | Do phones get native apps and store listings? This changes PRODUCT_SPEC §6 items 3 and 8 for the companion only; the desktop stays off the stores. | Yes, for the companion. | Without it, mobile stays a PWA and streams N1 to N6 don't start. |
| OD-2 | Mobile technology. | Option C (§4.3): a Tauri 2 app with the PWA's screens and the Rust protocol. | It fixes the stack and the layout for N1. |
| OD-3 | Layout additions: `apps/mobile`, `packages/remote`, and `keyring-core` for the mobile keychain. | Approve all three. | DECISIONS fixes the layout. |
| OD-4 | Build hosts. | The Azure Pipelines free tier now, plus a self-hosted runner if a Mac or Windows PC is around. After 1 November, GitHub minutes for macOS and Windows runs started by hand only. Making the repository public only as a business decision. | Nothing for macOS or Windows can be tested without a host. |
| OD-5 | Signing purchases. | The Apple Developer Program ($99 a year), which macOS notarization and every iOS build need. Windows: Artifact Signing ($9.99 a month) if eligible, else an OV certificate, else unsigned with honest steps. | Unsigned builds are a worse install and can't be in the App Store at all. |
| OD-6 | Is the phone companion Pro-only through the hosted API, as the spec says, or also in early access (the LAN relay, §4.7)? | Pro through the API for v1. The LAN relay later. | The PWA and the apps can't pair until the API is live. |
| OD-7 | Publish unsigned macOS and Windows early-access builds on the site after QA, or wait for signing? | Unsigned to the public `tandem-releases` prereleases for testers. On vibezeo.com only after QA and the owner's explicit OK. | The honesty rule (OPERATING §9). |
| OD-8 | Names and IDs: desktop `com.vibezeo.tandem` (keep); mobile `com.vibezeo.remote`; the store name "Vibezeo Remote". | Decide after the trademark search. | Store IDs and names are permanent or hard to change. |

---

## 7. HUMAN_TODO entries (ready to paste)

These follow the file's style. Each sits behind a mock or a stub, so no agent stream blocks on it.

### Product decisions waiting on the owner
- **Platforms (OD-1 to OD-8, `platforms-plan.md` §6).**
  - Approve, or change, the following:
    - native phone apps and store listings for the companion only (PRODUCT_SPEC §6 items 3 and 8);
    - Tauri 2 for the apps, with `apps/mobile`, `packages/remote` and `keyring-core`;
    - the build-host strategy;
    - the signing purchases;
    - Pro-only pairing;
    - whether unsigned desktop builds may be listed;
    - the mobile identifiers.
  - Verify: the decisions are recorded in BUILD_LOG and the spec diff is merged.

### Accounts & keys
- **Apple Developer Program** ($99 a year).
  - Enroll as an individual (the seller name is your legal name) or as an organization (needs a D-U-N-S number; the seller name is "Vibezeo").
  - As Account Holder, create:
    - a Developer ID Application certificate;
    - an Apple Distribution certificate;
    - an App Store Connect API key (App Manager).
  - Register the bundle IDs: `com.vibezeo.tandem` (desktop, for notarization) and the mobile ID from OD-8.
  - Create the App Store Connect app record, accept the agreements and complete EU trader status.
  - Create an APNs auth key (.p8) for push.
  - Secrets:
    - `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`;
    - `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH` (or `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`);
    - `APPLE_DEVELOPMENT_TEAM`;
    - on the API: `APNS_KEY_ID`, `APNS_TEAM_ID`, `APNS_PRIVATE_KEY`.
  - Verify: `security find-identity -v -p codesigning` on a Mac lists the Developer ID, and a test notarization of the `.dmg` succeeds.
- **Windows code signing.**
  - Check eligibility for Microsoft Artifact Signing: individuals in the US or Canada only; organizations in the US, Canada, EU, UK, Australia, New Zealand, Japan, South Korea, Singapore, Switzerland, Norway or Israel.
  - If eligible:
    - an Azure subscription;
    - an Artifact Signing account (Basic, $9.99 a month);
    - identity validation (1 to 20 business days);
    - a Public Trust certificate profile;
    - an app registration with the "Artifact Signing Certificate Profile Signer" role.
  - Secrets: `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERT_PROFILE`.
  - If not eligible: an OV code-signing certificate on a cloud HSM, or ship unsigned with SmartScreen steps.
  - Verify: `Get-AuthenticodeSignature Vibezeo_x.y.z_x64-setup.exe` reports Valid with a timestamp.
- **cargo-xwin licence.**
  - Agent sessions cross-build Windows with cargo-xwin, which downloads Microsoft's MSVC CRT and Windows SDK under Microsoft's licence (https://go.microsoft.com/fwlink/?LinkId=2086102). Accept it, or say no, and Windows builds happen only on Windows hosts.
  - Verify: BUILD_LOG records the answer.
- **Google Play Console** ($25, one time).
  - Complete identity verification.
  - Create the app with the package from OD-8.
  - Generate the upload keystore yourself (`keytool -genkey -v -keystore upload-keystore.jks -keyalg RSA -keysize 2048 -validity 10000 -alias upload`), keep it offline, and enroll in Play App Signing.
  - Secrets: `ANDROID_KEY_BASE64`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`.
  - Fill in Data safety and a privacy policy URL.
  - A personal account must recruit **12 testers opted in for 14 days in a row** before production. An organization account skips this.
  - Verify: the closed-test track shows 12 or more testers and day 14, and "Apply for production" is enabled.
- **Firebase project.**
  - FCM for push: put the service-account JSON on the API as `FCM_SERVICE_ACCOUNT`.
  - Optional: Test Lab, with a service account for agent sessions' `gcloud firebase test android run`.
  - Verify: one Robo test run of the debug APK passes.
- **Android developer verification.** Any APK offered outside Play needs the Android Developer Console. It's required in Brazil, Indonesia, Singapore and Thailand since 30 September 2026, and everywhere in 2027. Verify: the package name is registered.
- **Backend for phones.** This adds to the existing API items: Postgres, Redis, the domains `api.` and `app.vibezeo.com`, the keys and Stripe.
  - Deploy the API and the portal with the relay.
  - Serve `/.well-known/apple-app-site-association` and `/.well-known/assetlinks.json` from `app.vibezeo.com`.
  - If there's more than one API instance, fan revocations out through Redis (D-1003).
  - Verify: a hosted desktop build pairs with `https://app.vibezeo.com/remote` from a real phone, and the relay log holds no plaintext.

### Signing & distribution
- **Build hosts (OD-4).** Do one or more of these:
  - Link an Azure DevOps organization to an Azure subscription (the free tier: 1,800 Microsoft-hosted minutes a month, 60 minutes per run, macOS and Windows).
  - Register self-hosted runners (labels `macOS` and `Windows`) on machines you own.
  - Create a Codemagic individual account (500 macOS minutes a month).
  - Turn on Xcode Cloud (25 hours a month with the Apple membership).
  - After 1 November, keep GitHub's included minutes for macOS and Windows runs started by hand.
  - Verify: one green macOS job and one green Windows job (`cargo test --workspace`).
- **Attach `tauri-apps/nsis-tauri-utils` to agent sessions** (read access, `add_repo`). Tauri's NSIS bundler downloads a plugin from it, and the container's proxy blocks GitHub repositories that aren't attached. Verify: `tauri build --bundles nsis` gets past "downloading nsis_tauri_utils".

### Manual QA checklist (additions)
- **Platforms.** Windows SSH workspaces with Windows OpenSSH (no ControlMaster): connect, open a terminal, start a remote Claude thread, all with key auth through the Windows ssh-agent.
- **Platforms.** macOS Dictate under the hardened runtime: the microphone prompt shows the usage string, and a take records audio in a notarized build.
- **Platforms.** Vibezeo Remote on a real Android phone and a real iPhone:
  - scan the desktop's code;
  - see the agents;
  - deny an approval;
  - allow one while armed;
  - send to a thread;
  - type into an agent pane;
  - run a routine;
  - get a notification with the app closed;
  - unpair, after which the next command fails.

---

## 8. Roadmap and work streams

### 8.1 Phases

| Phase | When | What |
|---|---|---|
| **0. Container work and decisions** | Now to about 1 November | 1. Agent streams that need only Linux: W1, W2, M1, R1, P1, U1, the Rust and Android parts of N1. 2. The owner's decisions (§6). 3. Start the long human lead times: Apple enrollment; Artifact Signing eligibility and validation (1 to 20 business days); a Play account and testers (14 days); the API deployment. |
| **1. Desktop bring-up** | When a Mac or Windows host exists (Azure free tier now, GitHub minutes after 1 November, a self-hosted runner) | 1. M2 and W3: native builds and tests. 2. Unsigned early-access builds go to `tandem-releases` prereleases for testers, not vibezeo.com. 3. Manual QA. 4. Signing as identities arrive. 5. A site listing per OS through R1's gate. |
| **2. Companion live** | When the API is deployed | 1. The PWA, for Pro. 2. The native Android app (Play internal and closed tracks) and the iOS app (TestFlight). 3. Push. 4. Store submissions. |
| **3. Optional** | After the companion ships | SSH terminals on the phone (X1), the LAN relay (X2), PWA Web Push (X3), a tablet layout. |

### 8.2 Streams

Each stream is one Claude Code session on its own branch from `product-m0-m5`. Sizes are relative (S, M, L), not dates. Every stream follows CLAUDE.md:
- the tests come first and are named with the feature IDs;
- the gate is green before each commit;
- BUILD_LOG gets a section per stream;
- HUMAN_TODO gets every human step.

#### P0 · Spec and decisions (coordinator) · branch `platforms-spec` · S
- **Scope:** record OD-1 to OD-8 in BUILD_LOG. Amend:
  - PRODUCT_SPEC §5.6 (macOS 14+, Windows 10 22H2+, Android and iOS for the companion) and §6 (items 3 and 8);
  - new features with acceptance criteria:
    - F-RM-04 Phone notifications;
    - F-RM-05 Native companion app (pairing by scan or link, keys in the keychain, demo mode);
    - F-BE-13 Push relay;
  - DECISIONS (Platforms line, layout);
  - ARCHITECTURE §3 (companion IPC rows), §13.3 (push, native client), §17 (bundles per OS and the mobile build);
  - MILESTONES: a new M18 "Platforms" with E2E IDs;
  - `docs/REMAINING.md`.
- **Acceptance:**
  - The documents agree with each other (CONSISTENCY_REPORT pass).
  - `pnpm -r lint` and the docs tests are green.
  - The owner's approval is quoted in BUILD_LOG.
- **Depends on:** the owner's decisions.

#### P1 · Merge M16C (coordinator) · into `product-m0-m5` · S
- **Scope:**
  - Renumber M16C's decisions (D-1000 to D-1017 collide with the voice API's D-1000 to D-1008), for example to D-1050 and up, with every reference updated.
  - Merge, then run the gate and the E2E regression.
- **Acceptance:**
  - `grep -oE '^- D-[0-9]+:' BUILD_LOG.md | sort | uniq -d` prints nothing.
  - E2E-M16-4 passes 5/5, web @m16 2/2, and the full E2E regression passes.
- **Depends on:** nothing.

#### W1 · Windows readiness from Linux · branch `p-win-xcheck` · M
- **Scope:**
  - **Coordinate first:** check what `m11-windows-ready` holds.
  - **Tooling:** install cargo-xwin, link `clang-cl` to `clang-cl-18`, and add `scripts/agent/check-windows.sh` (`cargo xwin clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings`).
  - **Compile:** fix every `cfg(windows)` compile error in the workspace.
  - **PATH:** add the registry PATH merge behind a `RegistryEnv` trait with a fake (F-EN-01, ARCHITECTURE §5.2).
  - **Job Object:** move its assignment so no child can escape, or document why that's impossible with portable-pty and add a Windows-only test for W3.
  - **SSH:** add a Windows SSH path without ControlMaster (key auth through the Windows ssh-agent; `ssh -G` and `ssh-keyscan` stay), with option-builder unit tests.
  - **Docs:** OPERATING §3 and §5 document the setup.
- **Acceptance:**
  - `bash scripts/agent/check-windows.sh` is green. Without the licence, the MinGW GNU target type-checks.
  - New `f_en_01_*` and `f_rm_01_*` unit tests pass on Linux.
  - The gate is green.
- **Depends on:** the cargo-xwin licence answer (HUMAN_TODO).

#### W2 · Windows installer cross-build · branch `p-win-nsis` · M
- **Scope:**
  - Add `scripts/release-windows-cross.sh --dry-run`, the counterpart of `release-local.sh`. It checks the version, notices and sidecars (`build-sidecars.sh` learns `--runner cargo-xwin` and builds `tandem-hook` too), then runs `pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis --config src-tauri/tauri.sidecars.conf.json`.
  - Mark the output `-unsigned`, unless `jsign` signs it through `signCommand` with Artifact Signing secrets.
  - Write SHA256SUMS and a summary.
  - Teach `scripts/check-host-bundle.sh` to read an NSIS installer (`7z l`).
- **Acceptance:**
  - The script produces `dist/Vibezeo_<v>_x64-setup-unsigned.exe`, SHA256SUMS and `RELEASE_SUMMARY.md`, which says "unsigned; not yet tested on Windows".
  - `7z l` lists `tandem-tasks.exe`, `tandem-hook.exe` and the `host-binaries` manifest.
  - `check-host-bundle.sh` accepts it.
  - Nothing is published to the site.
- **Depends on:** W1, and access to `tauri-apps/nsis-tauri-utils` (HUMAN_TODO).

#### M1 · macOS readiness from Linux · branch `p-mac-config` · S
- **Scope:**
  - **Config:** `bundle.macOS.minimumSystemVersion: "14.0"`, `Info.plist` (`NSMicrophoneUsageDescription`), `Entitlements.plist` (`com.apple.security.device.audio-input`) and `bundle.macOS.entitlements`.
  - **Bundling:** add `tandem-hook` to `externalBin` (every OS) and `build-sidecars.sh`. `release.yml` signs `tandem-host` on macOS before bundling (`codesign --force --options runtime --timestamp`).
  - **Cross-check:** extend `check-cross.sh` to every crate with macOS code that type-checks without the SDK.
  - **Site:** the requirement reads "macOS 14 or later", the steps use Open Anyway (macOS 15), and the Windows card drops the MSI unless it's built on Windows.
  - **Docs:** fix the identifier text in DECISIONS and ARCHITECTURE.
- **Acceptance:**
  - A test pins `minimumSystemVersion`, the usage string and the entitlement.
  - `check-cross.sh` is green for the listed crates.
  - The site tests are updated and green.
  - The gate is green.
- **Depends on:** nothing.

#### R1 · Release and site gate · branch `p-release-gate` · S
- **Scope:**
  - **Data:** `apps/web/lib/release.ts` gains a per-OS QA record (`{date, machine, osVersion, checklist}`) and store links for phones (`store: "play" | "app-store"`, with a `beta` flag).
  - **Site tests** fail if an OS is `available` without:
    - its file in `public/downloads`;
    - a SHA256SUMS line;
    - a QA record.
    - A store badge without a live-listing flag fails too.
  - **Artifacts:** `scripts/check-release-artifacts.ts` verifies the checksums and, for signed builds, the signature evidence that CI attaches (`spctl` and `stapler` output, `Get-AuthenticodeSignature` output, `apksigner` output).
  - **Changelog:** `check-changelog.ts` accepts `[and]` and `[ios]`.
  - **Docs:** OPERATING §6 covers the per-OS release steps.
- **Acceptance:**
  - The site tests prove each refusal.
  - A dry run with the Linux .deb passes.
  - The gate is green.
- **Depends on:** nothing.

#### C1 · Free build hosts · branch `p-ci-hosts` · M
- **Scope:**
  - **Workflows:**
    - pull requests no longer spend hosted minutes (Linux CI runs by hand or on a self-hosted label);
    - `release.yml` and a new `platforms.yml` (tests plus a debug build per OS) can target `[self-hosted, macOS]` and `[self-hosted, Windows]`, or GitHub-hosted runners by hand.
  - **Azure Pipelines:** `azure-pipelines.yml` mirrors the macOS and Windows jobs. Each job stays under 60 minutes: aarch64 and x86_64 macOS jobs plus a `lipo` job.
  - **Codemagic:** optional `codemagic.yaml` for iOS.
  - **Probe:** run one job on a self-hosted runner under the current billing state.
- **Acceptance:**
  - One green macOS job and one green Windows job, each running `cargo test --workspace` and `pnpm --filter desktop tauri build --debug --no-bundle`.
  - The probe result is recorded.
  - OPERATING §8 documents the hosts.
- **Depends on:** a human sets up a host (HUMAN_TODO).

#### M2 · macOS native bring-up · branch `p-mac-native` · L
- **Runs on:** a Mac host (CI, or a Claude Code session on the owner's Mac).
- **Scope:**
  - Fix what fails natively. Expect issues in code that was only type-checked: `insert/macos.rs` (`objc_msgSend`), `hotkey/macos.rs`, `browser/wry_host.rs`, the launchd launcher and detection.
  - `cargo test --workspace` on macOS.
  - The universal build, plus a smoke script that launches the app and takes a screenshot of the welcome screen.
  - An E2E subset (startup, terminals, threads, settings) through `@wdio/tauri-service` with `tauri-plugin-wdio-webdriver` behind a test-only feature, with `check-release-harness.sh` extended.
  - Signing and notarization once the secrets exist.
- **Acceptance:**
  - The tests are green on macOS 14 or 15.
  - `lipo -info` shows both architectures for the app, the sidecars and `tandem-host`.
  - `codesign --verify --deep --strict` passes.
  - With secrets: `spctl -a -vv` reports "Notarized Developer ID" and `xcrun stapler validate` passes.
  - The smoke test and the E2E subset are green.
  - BUILD_LOG records the run.
- **Depends on:** M1 and C1, plus Apple (for signing only).

#### W3 · Windows native bring-up · branch `p-win-native` · L
- **Runs on:** a Windows host.
- **Scope:**
  - `cargo test --workspace` on Windows, with Windows-only tests:
    - the Job Object tree kill;
    - the ConPTY resize after first output;
    - the registry PATH merge against the real registry under a test key;
    - a `schtasks` round trip in a temporary folder;
    - WSL detection when a distro exists.
  - `tauri build --bundles nsis,msi`.
  - E2E through `tauri-driver`, skipping the specs that need Linux tools (Xvfb goldens, zsh and fish).
  - Signing once possible.
- **Acceptance:**
  - The tests are green on Windows Server 2022 or 2025.
  - The installers build.
  - The E2E subset is green.
  - With secrets: `Get-AuthenticodeSignature` reports Valid for the exe, the sidecars and the installer.
  - BUILD_LOG records the run.
- **Depends on:** W1 and C1, plus Windows signing (for signing only).

#### A1 · Hosted backend for phones · branch `p-api-deploy` · M (mostly human)
- **Scope:**
  - Deploy the API and portal (HUMAN_TODO).
  - Fan revocations out through Redis if there's more than one instance (D-1003).
  - Add `/.well-known/apple-app-site-association` and `assetlinks.json` routes to `apps/web`, with tests.
  - Health checks.
- **Acceptance:**
  - `GET https://api.vibezeo.com/v1/health` returns 200.
  - A relay upgrade succeeds for a live pairing and is refused with 1008 otherwise.
  - A hosted desktop build pairs with the PWA from a real phone.
  - The production log configuration still keeps frame bytes out (the E2E-M16-4 grep, run on a log sample).
- **Depends on:** P1 and HUMAN_TODO (backend).

#### U1 · Shared companion UI · branch `p-remote-ui` · M
- **Scope:**
  - Add `packages/remote`, with the `CompanionBackend` interface and the PWA's screens moved there.
  - Add `WebCompanion` (today's `PhoneClient` with the IndexedDB store).
  - `apps/web/app/remote` uses the package.
  - Every screen has empty, loading, error and populated states (CLAUDE.md UI rule).
- **Acceptance:**
  - vitest passes for every screen with a fake backend.
  - web @m16 passes 2/2.
  - There's no visible change.
  - The gate is green.
- **Depends on:** P1 and OD-3.

#### N1 · Mobile app shell, Rust protocol and the Android build · branch `p-mobile-shell` · L
- **Scope:**
  - **App:** `apps/mobile` (Vite, React, `packages/remote`) and `apps/mobile/src-tauri` (crate `tandem-mobile`), with `NativeCompanion` calling Rust.
  - **Protocol:** `tandem_core::remote::phone` (the client state machine).
  - **Relay socket:** tokio-tungstenite.
  - **Keychain:** `keyring-core` with the Apple and Android stores behind `Keychain`, and the in-memory fake for tests.
  - **IPC:** the rows go into ARCHITECTURE first.
  - **Android:** the toolchain set up in the container, plus a debug APK.
- **Acceptance:**
  - **Protocol tests:** `f_rm_03_*` Rust tests against `remote_v1.json` and a fake relay cover the pairing tag, welcome, a command round trip, a refused replay, a rekey during a dropped connection, and `bye` wiping the keys.
  - **Build:** `pnpm --filter mobile tauri android build --apk --target aarch64` succeeds in the container.
  - **Checks:** `zipalign -c -P 16 -v 4` passes, `llvm-readelf -l` shows align 0x4000, and `apksigner verify` passes.
  - **Optional:** a Test Lab Robo run reaches the pairing screen.
  - **Gate:** the gate builds and tests the mobile Rust crate for the host without the Android SDK.
- **Depends on:** U1, OD-1 to OD-3.

#### N2 · Native pairing · branch `p-mobile-pairing` · M
- **Scope:**
  - An in-app scanner (barcode-scanner).
  - App links and universal links for the existing pairing URL.
  - The fragment is removed from history and logs.
  - An optional biometric check before an allow or before typing, behind a setting, with a fake in tests.
- **Acceptance:**
  - Unit tests: link parsing, an invalid link reaching the error state, the fragment never logged.
  - Manual: scan on a real Android phone and an iPhone, and both pair.
- **Depends on:** N1, plus A1 (the association files).

#### N3 · Push notifications · branch `p-push` · L
- **Scope:**
  - F-RM-04 and F-BE-13 per §4.5.
  - API routes with `MockPush`.
  - Desktop sealing and the opt-in setting.
  - A small Tauri plugin: Kotlin `FirebaseMessagingService` and a Swift Notification Service Extension that decrypt.
  - The subprocessor list updated.
- **Acceptance:**
  - **API tests:** the API forwards opaque bytes, has no crypto imports (lint rule), applies the rate limit and collapse, and refuses a revoked pairing.
  - **Desktop tests:** nothing is sent without opt-in, and there's no message text unless "Include message preview" is on.
  - **Seal and open:** the vectors match in Rust, and on Android and iOS through the plugin's unit tests.
  - **Manual:** with the app closed, the decrypted notification arrives on both platforms. With the key missing, the generic text shows.
- **Depends on:** N1 and A1. Human: the APNs key and Firebase.

#### N4 · Demo mode and store readiness · branch `p-mobile-store` · M
- **Scope:**
  - A built-in demo with a simulated desktop, labelled as a demo and offline, like `apps/desktop/src/demo`.
  - Store listing copy that only claims what ships, with screenshots from the real app.
  - A privacy policy section for the app.
  - Draft answers for Play's Data safety form and App Store Connect's App Privacy, plus an export-compliance draft (standard algorithms: X25519, HKDF-SHA256, XChaCha20-Poly1305), a privacy manifest and reviewer notes.
- **Acceptance:**
  - The demo covers every screen and is tested in vitest.
  - The copy passes the honesty rule (OPERATING §9) in review.
- **Depends on:** U1 and N1.

#### N5 · Android release · branch `p-android-release` · M
- **Scope:**
  - Release signing from secrets and the AAB (`tauri android build --aab`).
  - The internal track (the owner's first upload is manual), then the closed test, then production.
- **Acceptance:**
  - Play accepts the AAB (target API 36 or later, 16 KB).
  - The pre-launch report is clean.
  - The closed test runs 12 testers for 14 days.
  - Production access is granted.
- **Depends on:** N1 to N4, plus the Google account.

#### N6 · iOS build and TestFlight · branch `p-ios` · L
- **Runs on:** a Mac host.
- **Scope:**
  - `tauri ios init` (commit `gen/apple`) and an Xcode 26 build.
  - The Notification Service Extension target, the privacy manifest and the export-compliance key.
  - CI on Codemagic, Xcode Cloud or a GitHub macOS runner.
  - The TestFlight upload, then review submission with the demo path.
- **Acceptance:**
  - The IPA builds on CI.
  - altool uploads it with no ITMS warnings.
  - A TestFlight install pairs with a desktop.
  - Push works.
  - App Review approves it.
- **Depends on:** N1 to N4, Apple, and a Mac host.

#### X1 to X3 · Optional (Phase 3)
- **X1 · SSH terminals on the phone** · `p-mobile-ssh` · L.
  - Scope: the russh transport for `HostClient`, uploading and attaching `tandem-host`, "Send host to phone", and the terminal with a key row.
  - Acceptance: F-RM-01's AC1 to AC5 pass on Linux against the Docker `sshd` fixture with the russh transport (connect and upload, `echo hi`, a refused host-key change, a refused path escape, no local provider-home paths in the remote environment, reattaching the same remote PID).
  - Shared: the same transport can replace ControlMaster on Windows.
- **X2 · LAN relay** · `p-lan-relay` · M.
  - Scope: the desktop hosts the relay with a pinned self-signed TLS certificate.
  - Acceptance: the relay tests reuse F-BE-11.1, and a phone on the same Wi-Fi pairs without an account. Needs OD-6.
- **X3 · PWA Web Push** · `p-pwa-push` · M.
  - Scope: the desktop acts as the application server (VAPID, RFC 8291).
  - Acceptance: tests against RFC 8291 vectors, and an iOS 16.4+ Home Screen app receives a push.

### 8.3 Dependencies

```
owner decisions ─► P0 ─────────────────────────────┐
P1 (merge M16C) ─► U1 ─► N1 ─► N2 ─┐               │
                         │    N3 ◄─┼── A1 (API live, AASA, assetlinks) ◄─ HUMAN: backend
                         └──► N4 ──┴► N5 (Play) ◄── HUMAN: Google
                                     N6 (iOS)  ◄── HUMAN: Apple + a Mac host (C1)
W1 ─► W2 (unsigned installer, container)
W1 ─► W3 (Windows host) ◄─ C1 ◄─ HUMAN: hosts      W3 signing ◄─ HUMAN: Windows signing
M1 ─► M2 (Mac host)     ◄─ C1                       M2 notarization ◄─ HUMAN: Apple
R1 ─► every site listing (macOS, Windows, Play, App Store)
```

**Can start today, in parallel, in separate containers:** P0 and P1 (coordinator), W1, M1, R1, the preparation part of C1, and the Android toolchain spike from N1. U1 and the rest of N1 start after P1.

### 8.4 Rules for every stream

- **Decision ranges** (the coordinator confirms; checked free on 10 October; the highest in use is D-1049, on the M16C branch):
  - D-1100 to D-1149: desktop platform streams (W1 to W3, M1, M2, C1, R1);
  - D-1150 to D-1199: mobile (U1, N1 to N6);
  - D-1200 to D-1249: optional streams.
- **Shared files:** BUILD_LOG, `en.json`, ARCHITECTURE and the command registry get small merge conflicts. Merge `product-m0-m5` often.
- **Nothing outward-facing without the owner:** no publishing, no store submissions, no `main` merges.
- **No false claims:** nothing that isn't built and tested goes on the site, in a store listing or in the CHANGELOG.
- **Disk:** an Android toolchain plus `target/` won't fit next to a full desktop `target/` (8 GB free). Prune first (`python3 scripts/agent/prune-target.py`, `CARGO_INCREMENTAL=0`) or keep mobile work in its own container.

---

## 9. Risks and unknowns

1. **macOS and Windows have never been compiled natively.** Expect a bring-up cost: compile errors in code that was only type-checked, and behaviour bugs in WKWebView and WebView2 (CSS, xterm's WebGL renderer, IME, child webviews).
2. **Native C and C++ dependencies:**
   - whisper.cpp (CMake) and ONNX Runtime (`ort` 2.0.0-rc.13 prebuilt libraries) under the universal macOS build and the MSVC cross-build;
   - a dynamically linked ONNX Runtime library would need signing too;
   - whisper's Metal feature isn't enabled (CPU only on Macs), which matters for the M1 latency target.
3. **Notarization surprises:** unsigned Mach-O files in resources, entitlement mistakes.
4. **The Windows SSH transport** needs a non-ControlMaster path (gap 6). Testing it needs a reachable Linux sshd from a Windows host, which GitHub's Windows runners can't run in Docker. Manual QA covers it until X1.
5. **The billing block may also stop self-hosted runners** on this account. Probe it in C1.
6. **Tauri mobile is younger** than React Native or Capacitor. Push needs our own native code (Kotlin and Swift), and an iOS extension target added to the generated Xcode project.
7. **The hosted API isn't deployed.** Every phone path waits on it unless OD-6 picks the LAN relay.
8. **App Review:** a companion that needs a desktop (4.2.3(i)), a demo mode that needs prior approval (2.1), and anything that mirrors desktop terminals (4.2.7).
9. **Google Play:** the 12-tester, 14-day closed test (personal account), target-API deadlines every August, and 16 KB alignment.
10. **Android developer verification goes global in 2027.** Any APK on the site needs it.
11. **Container constraints:** about 8 GB of free disk, no KVM, and GitHub downloads limited to attached repositories.
12. **Parallel sessions collide on decision numbers** (gap 12). Ranges must be assigned before work starts.
13. **Names:** the trademark check on "Vibezeo" before store listings, because store names are public and sticky.

---

## 10. Sources

**Repository:**
- `CLAUDE.md`, `BUILD_LOG.md` ("Current state"), `build-kit/DECISIONS.md`, `build-kit/ARCHITECTURE.md` (§1, §2.4, §2.9 to §2.12, §5, §10.3, §12, §13, §17), `build-kit/PRODUCT_SPEC.md` (F-RM-01 to F-RM-03, F-BE-11, §5.6, §6), `build-kit/MILESTONES.md` (M16C), `HUMAN_TODO.md`, `docs/OPERATING.md`, `docs/REMAINING.md`;
- `.github/workflows/{ci,release}.yml`, `apps/desktop/src-tauri/tauri*.json`, `scripts/{release-local,build-sidecars}.sh`, `scripts/ci/build-tandem-host.sh`, `scripts/agent/check-cross.sh`, `apps/web/lib/release.ts`;
- the `cfg(target_os)` modules in `crates/tandem-{pty,voice,routines,host,engines,core}` and `apps/desktop/src-tauri`;
- `git show origin/m16c-phone-companion:…`;
- GitHub Actions run and job history (`gh api`).

**Tauri:**
- Windows installer and cross-compiling: https://v2.tauri.app/distribute/windows-installer/
- Windows signing: https://v2.tauri.app/distribute/sign/windows/
- macOS signing and notarization: https://v2.tauri.app/distribute/sign/macos/
- Prerequisites (Android, iOS): https://v2.tauri.app/start/prerequisites/
- Google Play: https://v2.tauri.app/distribute/google-play/
- App Store: https://v2.tauri.app/distribute/app-store/
- iOS signing: https://v2.tauri.app/distribute/sign/ios/
- Android signing: https://v2.tauri.app/distribute/sign/android/
- Notification plugin: https://v2.tauri.app/plugin/notification/
- Plugin platform table: https://github.com/tauri-apps/plugins-workspace (README)
- WebDriver: https://v2.tauri.app/develop/tests/webdriver/
- Config schema: https://schema.tauri.app/config/2
- Android template: https://github.com/tauri-apps/tauri/tree/dev/crates/tauri-cli/templates/mobile/android

**Microsoft:**
- Artifact Signing quickstart (eligibility): https://learn.microsoft.com/azure/artifact-signing/quickstart
- Pricing: https://azure.microsoft.com/en-us/pricing/details/artifact-signing/
- Azure Pipelines free tier: https://learn.microsoft.com/en-us/azure/devops/pipelines/licensing/concurrent-jobs
- Microsoft Store free registration: https://blogs.windows.com/windowsdeveloper/2025/09/10/free-developer-registration-for-individual-developers-on-microsoft-store/

**Cross-building and signing tools:**
- cargo-xwin: https://github.com/rust-cross/cargo-xwin
- rcodesign notarization: https://gregoryszorc.com/docs/apple-codesign/main/apple_codesign_rcodesign_notarizing.html
- jsign 7.0 (Trusted Signing): https://newreleases.io/project/github/ebourg/jsign/release/7.0
- Win32-OpenSSH without multiplexing (secondary): https://stackharbor.com/en/knowledge-base/ssh-connection-multiplexing-controlmaster/ and https://docs.alliancecan.ca/wiki/Special:MobileDiff/151597

**Apple:**
- App Review Guidelines: https://developer.apple.com/app-store/review/guidelines/
- Xcode Cloud: https://developer.apple.com/xcode-cloud/get-started
- SDK minimum from 28 April 2026 (secondary): https://9to5mac.com/?p=1037785 and https://expo.dev/blog/app-store-connect-minimum-sdk-26.md
- Privacy manifest enforcement (secondary): https://bitrise.io/blog/post/enforcement-of-apple-privacy-manifest-starting-from-may-1-2024
- macOS 15 Gatekeeper change (secondary): https://appleinsider.com/inside/macos-sequoia/tips/whats-changed-in-runtime-protection-for-macos-sequoia
- Hardened-runtime microphone entitlement: https://developer.apple.com/forums/thread/741303 and https://bugs.openjdk.org/browse/JDK-8244951
- Web Push for Home Screen apps: https://webkit.org/blog/13878/web-push-for-web-apps-on-ios-and-ipados/
- EU trader status (secondary): https://www.techrepublic.com/article/eu-app-store-apple-digital-services-act/

**Google:**
- Target API: https://developer.android.com/google/play/requirements/target-sdk
- 16 KB pages: https://developer.android.com/16kb-page-size
- Closed testing for new personal accounts: https://support.google.com/googleplay/android-developer/answer/14151465
- Developer verification: https://developer.android.com/developer-verification/guides
- Firebase Test Lab quotas: https://firebase.google.com/docs/test-lab/usage-quotas-pricing

**Keychain crates:**
- keyring v3 platforms: https://docs.rs/keyring/3.6.3/keyring/
- keyring 4 and stores: https://docs.rs/crate/keyring/4.2.0
- android-native-keyring-store: https://docs.rs/crate/android-native-keyring-store/latest

**CI:**
- GitHub Actions billing: https://docs.github.com/en/billing/concepts/product-billing/github-actions
- 2026 pricing changes: https://github.com/resources/insights/2026-pricing-changes-for-github-actions
- Codemagic pricing: https://codemagic.io/pricing/
- CircleCI pricing: https://circleci.com/pricing/
- Cirrus CI shutdown: https://cirruslabs.org/

**Push plugins (community):**
- https://github.com/yanqianglu/tauri-plugin-mobile-push
- https://github.com/Choochmeque/tauri-plugin-notifications
