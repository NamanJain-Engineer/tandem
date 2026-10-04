# BUILD PROMPT: paste this whole file to your coding agent

> Usage: put the `build-kit/` folder somewhere the agent can read, open an agent session (Claude Code, Codex, etc.) in an **empty directory**, and say:
> **"Read build-kit/BUILD_PROMPT.md and follow it until the product is complete."**
> If the session ends or runs out of context, start a new one with the same sentence. The agent resumes from `BUILD_LOG.md`.

---

You are the sole engineer building **Tandem**, a cross-platform desktop "agent workspace" for building software with AI coding agents. You will build the complete product autonomously, milestone by milestone, without asking the human questions. Everything you need is in `build-kit/`.

## 1. Documents (read in this order before writing any code)
1. `build-kit/DECISIONS.md`: **binding** choices for names, stack, repo layout, business model, visual identity and engineering rules. Do not re-decide them.
2. `build-kit/PRODUCT_SPEC.md`: what to build. Every feature has an ID (`F-AREA-nn`), a priority and acceptance criteria. It is the source of truth for behaviour.
3. `build-kit/ARCHITECTURE.md`: how to build it. Crates, IPC commands and events, data model, API, engine adapter protocols, gateway, flows.
4. `build-kit/UI_SPEC.md`: design tokens, components, every screen with wireframes and microcopy.
5. `build-kit/MILESTONES.md`: the ordered plan. Each milestone lists tasks and **verification commands** that must pass.
6. `build-kit/reference/` (optional, background only): research on an existing product in this category. Use it only to understand intended behaviour when the spec is ambiguous. **Never copy its names, branding, copy or code.**

If documents conflict, the precedence is DECISIONS > PRODUCT_SPEC > ARCHITECTURE > UI_SPEC > MILESTONES, except that the build *order* is MILESTONES (PRODUCT_SPEC §7.2). Log the conflict and your resolution in `BUILD_LOG.md`. `build-kit/CONSISTENCY_REPORT.md` records the cross-document reconciliations already applied. Ignore `build-kit/_drafts/` (superseded drafts of UI_SPEC).

## 2. Operating loop (repeat until every milestone is DONE)
1. **Resume.** If `tandem/BUILD_LOG.md` exists, read it along with `HUMAN_TODO.md`, run `git log --oneline -20`, and continue from the first milestone that isn't DONE. Otherwise start at M0 and create the repo per DECISIONS.
2. **Plan the milestone.** Write a short task checklist for it at the top of `BUILD_LOG.md`, mapping each task to feature IDs and acceptance criteria.
3. **Implement in small vertical slices.** For each slice:
   - Write or extend tests first from the acceptance criteria. Rust goes in `cargo test`, UI in `vitest`, web in `playwright`, desktop E2E in `wdio` + `tauri-driver` under `xvfb-run`.
   - Implement until the tests pass.
   - Run the milestone's verification commands plus the global gate G (`pnpm gate`; DECISIONS rule 1, MILESTONES §0):
     ```bash
     pnpm -r lint && pnpm -r test && cargo test --workspace && pnpm --filter desktop build
     cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check
     pnpm gen:protocol && git diff --exit-code packages/protocol
     ```
   - Commit with a message like `M3: F-CD-02 split/snap panes`.
4. **Close the milestone.** Every verification command passes and every P0/P1 acceptance criterion in scope has an automated test, or is marked `[manual]` with a note describing the check you did. Mark the milestone DONE in `BUILD_LOG.md` with a summary, then `git tag m<N>`.
5. Move to the next milestone. **Never stop to ask the human.** Decide, log the decision, and continue.

## 3. When you are blocked
- **Needs a human-owned account or secret** (Apple or Windows signing, live Stripe, OAuth client IDs, OpenAI or xAI keys, SMTP, a domain): implement it behind an interface with a **dev/mock provider** so everything runs locally. Add a precise entry to `HUMAN_TODO.md` saying what to create, where to put the value (env var or keychain name) and how to verify it. Then continue.
- **Needs a third-party CLI that isn't installed** (e.g. `claude`, `codex`): build the adapter against the documented protocol, and test it with a **fake CLI fixture** that replays recorded JSON streams (`crates/tandem-engines/tests/fixtures/`). If the real CLI is on PATH, also run the gated integration test (`TANDEM_REAL_ENGINES=1`).
- **Hardware or OS features you can't exercise** (microphone, global hotkeys, macOS-only APIs, the iOS Simulator): implement them, unit-test the logic with injected fakes, and mark the physical check `[manual]` in `HUMAN_TODO.md`.
- **A test is flaky or a dependency fails:** fix the root cause. Don't skip tests or weaken assertions. If a library can't do the job after two real attempts, pick an alternative, log why, and continue.
- **Spec ambiguity:** choose the simplest behaviour consistent with the acceptance criteria and the UI spec, then log it under "Decisions" in `BUILD_LOG.md`.

## 4. Quality bar
- The product must actually work end to end on Linux in dev. You must be able to:
  - `pnpm dev` (runs infra via docker compose, the API, the web app and the desktop app);
  - sign up;
  - open a folder;
  - launch a real shell in 16 panes;
  - start an engine thread (real CLI or fixture);
  - schedule a routine;
  - dictate text;
  - see the entitlement gate work on a free account.
- There is no placeholder UI in shipped screens. Every screen in UI_SPEC exists with its empty, loading, error and populated states.
- Rules from DECISIONS §"Non-negotiable engineering rules" apply at all times: secrets only in the keychain, nothing leaving the machine by default, server-side entitlements.
- Keep `README.md` current: setup, `pnpm dev`, the test commands, and the release process.

## 5. Context management (for long runs)
- `BUILD_LOG.md` is your memory. After every slice, append: what changed, what's next, open issues. Keep a "Current state" section at the top, always accurate, under 40 lines.
- Don't paste large files into context. Search and read only what you need.
- If your tool supports sub-agents, you may parallelise independent slices within a milestone (e.g. UI components vs Rust crate). Integrate and run the global gate yourself before committing.

## 6. Definition of done (the whole product)
- All milestones in `MILESTONES.md` are DONE and tagged.
- All P0 and P1 acceptance criteria have passing automated tests, or documented `[manual]` checks.
- CI workflow files exist and pass locally (`act`, or the equivalent commands).
- `HUMAN_TODO.md` lists every remaining human-only step, grouped as: *Accounts & keys*, *Signing & distribution*, *Legal*, *Manual QA checklist*.
- The final `BUILD_LOG.md` entry summarises: features shipped (by ID), known limitations, and next steps for the P2 features not done.

Begin now: read the documents in §1, then execute the loop in §2.
