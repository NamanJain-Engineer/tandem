# BUILD_LOG

## Current state
- Repo: https://github.com/NamanJain-Engineer/tandem (private, owner NamanJain-Engineer).
- Milestone: M0 in progress. Not tagged.
- Done: `tandem-core`, `tandem-store`, `@tandem/protocol`, `api` (`GET /v1/health`, problem details, header redaction), `web` (`GET /api/health`), `@tandem/ui` Button, `infra/docker-compose.yml`, CI workflow.
- Next: Tauri desktop splash (UI-02), `pnpm dev` bringing web up beside the API, and a tracing subscriber on the Rust logger.
- Gate: `pnpm -r lint`, `pnpm -r test`, `cargo test --workspace`, clippy, rustfmt. Docker is not available in this environment, so compose is written but not started here.

## Decisions
- D-1: F-CD-09 owned by M9. Needs the files editor and browser.
- D-2: F-SH-08 framework slice in M5, accounts step in M6, owned by M8.
- D-3: F-DI-09 owned by M8 so dictionary actions exist before enhance interplay.
- D-4: M7 builds the gateway skeleton early so thread approvals are real.
- D-5: M7 ships a mock auto-title route; the real provider lands in M13.
- D-6: Dashboard approval is tested on the broker first, then again with a real thread.
- D-7: Thread interrupted state is tested on the sessions table in M5 and in the UI in M7.
- D-8: Create-with-AI is tested against an API fixture in M10.
- D-9: Threat-model tests land beside each mechanism; M15 audits completeness.
- D-10: Test-only dependencies do not change the runtime stack.
- Deviation: production OS keychain (`keyring`) is not linked yet. `MemoryKeychain` is selected when `TANDEM_FAKE_KEYCHAIN=1`. System keychain is the next core task so this slice builds on Linux without a secret-service daemon.

## Log
- M0: F-TL-02 and F-TL-03 core, plus the early slices of settings, commands, and sanitized logs.
- M0: API `GET /v1/health`, web `GET /api/health`, protocol types, Button, compose file, CI.
