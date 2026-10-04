# Tandem: rules for coding agents

## Always
- Read `BUILD_LOG.md` ("Current state") before working. Update it after every slice.
- Follow `DECISIONS.md` once it is copied into this repo. Until then, follow the choices recorded in `BUILD_LOG.md`.
- Write tests from the acceptance criteria before implementing, and reference the id in the test name.
- Run `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --all --check` before every commit.
- Make small commits: `M<n>: F-XX-nn <what>`.

## Never
- Use names, branding, copy, or code from other products in this category.
- Store secrets anywhere but the OS keychain. The in-memory keychain is for tests (`TANDEM_FAKE_KEYCHAIN=1`) only.
- Log tokens, prompt text, or file contents. Log lines go through the redactor.
- Send data off the machine without an explicit user opt-in.
- Read, copy, or proxy AI-vendor OAuth tokens. Engines authenticate inside their own provider home.
- Skip, delete, or weaken a failing test to go green.

## Paths
- App data: `~/.tandem/` (overridable with `TANDEM_HOME`).
- Provider homes: `~/.tandem/providers/<engine>/<account>/`.
- Worktrees: `<repo>/.tandem/worktrees/<slug>`, on branches `tandem/<slug>`.
