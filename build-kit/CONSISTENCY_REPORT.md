# CONSISTENCY_REPORT: cross-document reconciliation pass

**Scope:** BUILD_PROMPT.md, DECISIONS.md, CLAUDE.md, PRODUCT_SPEC.md, ARCHITECTURE.md, UI_SPEC.md, MILESTONES.md. `reference/` is exempt; `_drafts/` wasn't edited because it's superseded (see Open items).
**Rule applied:** DECISIONS > PRODUCT_SPEC > ARCHITECTURE > UI_SPEC > MILESTONES. Where a lower document had made a deliberate, better reconciliation, that choice was promoted into the higher document. Each such case is marked **(promoted)**.
**Format:** each bullet reads *File · section: before → after (reason)*. Headings 1–15 match the 15 known issues; the other headings cover extra contradictions found during the pass.

**Verification run after all edits**
- MILESTONES §20.1 coverage script: 152 spec IDs = 152 owned, 0 missing or duplicated, P2 only in M16, and the §20 table equals the `**Owns:**` lines.
- No `tn/` branch prefix remains anywhere.
- Every `UI-nn` reference resolves to the UI_SPEC §8 index (83 screens).
- `Alt+Space` appears only in "not bound on Windows/Linux" statements.
- Every `/v1/...` path in the kit exists in ARCHITECTURE §14.6. The exceptions are legitimate: the web route `/api/v1/site-info`, the local status server's `/v1/status` and `/v1/askpass`, and third-party URLs.
- The product-name grep is clean outside DECISIONS' prohibition list.

---

## 1. Status colours
- PRODUCT_SPEC F-SH-03 rail row: "Working n (green) / Needs you (amber) / Failed (red)" → `status-working` / `status-waiting` / `status-error`.
- PRODUCT_SPEC F-CD-12 status dot: "green=working, amber=waiting, blue=done, grey=idle, red=failed" → UI_SPEC token names (`status-working` sky, `status-waiting` amber, `status-done` green, `status-idle` slate, `status-error` rose) with §1.3 shapes.
- PRODUCT_SPEC F-DB-01 stacked bar "waiting=amber, working=green, done=blue, idle=grey" → `status-*` tokens; waiting card "amber border" → `status-waiting` border.
- PRODUCT_SPEC F-CN-04 ring "speaking=accent, thinking=violet, error=amber" → UI_SPEC §2.30 WaveformRing states (listening accent, thinking rotating accent highlight, speaking sky, error rose).
- PRODUCT_SPEC F-AC-04/F-AC-06 "amber/red" → `warning`/`danger`; UI_SPEC §1.3 meter thresholds 70/90 → 80/95 to match PRODUCT_SPEC acceptance (75% neutral, 85% warning). MILESTONES M4 E2E-M4-3 and M6 task 8 / E2E updated to token names.
- UI_SPEC §8.1 items 1 and 2 marked resolved.
## 2. Contrast
- UI_SPEC §1.2: dark `--tn-text-subtle` #6b7a90 → #7d8ba0; light `--tn-accent` teal-600 → teal-700 (#0f766e), hover teal-700→teal-800, press teal-800→teal-900, accent-soft/ring/selection rgb(13 148 136) → rgb(15 118 110); added `--sky-700: #0369a1`; added `--tn-status-{working,waiting,done}-text` (dark = same as shape tokens; light = sky-700/amber-700/green-700) plus the Tailwind `@theme` mappings `--color-*-text`.
- UI_SPEC §1.3: new row "Status as text" using `status-*-text`.
- UI_SPEC §7.3 table: header "Required fix" → "Fix (applied in §1.2)"; failing rows marked fixed. §8.1 item 7 marked resolved.
- DECISIONS Visual identity: accent "#2DD4BF" → "#2DD4BF (dark) / #0F766E (light)" + pointer to UI_SPEC §1.2 status tokens. PRODUCT_SPEC F-SH-05 now points to UI_SPEC §1.2 and names both accents.
## Other: rail and sidebar sizes (UI_SPEC §8.1 item 8)
- UI_SPEC §1.2 layout constants, §3.1 diagram, §3.3, §3.5: rail min 200→180px; sidebar 440 (320–720) → 470 (320–900, or full-window) per PRODUCT_SPEC F-SH-03/F-SH-10. §8.1 item 8 resolved. MILESTONES M5 note "PRODUCT_SPEC wins over UI_SPEC's 440" → "UI_SPEC agrees".
## 3. Shortcuts
- PRODUCT_SPEC F-SH-09: states UI_SPEC §5 is the canonical complete map; "Settings → Keyboard, P2" → "Settings › Shortcuts (UI-113); app-shortcut rebinding P2"; "Launch wizard" labelled launch sheet UI-12; added rows Mod+Shift+N/J, Mod+Alt+K, Mod+Alt+Shift+D, Mod+Alt+E, Mod+Shift+M, Mod+Shift+., global Dictate per OS; row "Dictate (in-app composer, hold) Alt+Space" → global Dictate key, or Option+Space on macOS only; passthrough rule now includes Ctrl+Alt+… combos, Ctrl+Tab/Ctrl+Shift+Tab, F6 (UI_SPEC §5 list). (A "New window" row was briefly added and then removed; see §13 multi-window.)
- PRODUCT_SPEC F-DI-12: "`Alt+Space` held" → global Dictate key (or Option+Space on macOS); Alt+Space unbound on Win/Linux.
- PRODUCT_SPEC F-TH-11 keyboard: added Mod+Shift+A focus, A / D / E keys (card-focused), aligned with UI_SPEC.
- UI_SPEC §5: composer-dictation row → global Dictate key (+⌥Space mac), Alt+Space not bound; approval rows expanded (⇧⌘A focus; A/⌘↩ allow once; ⇧⌘↩ allow for thread; D; E; Esc); Reconciliations block rewritten as "applied" with the `Alt+Space` decision and the Mod+Shift+Enter scoping note. §2.24 and UI-23 keyboard lines aligned; UI-68 line aligned; §8.1 item 3 resolved.
- MILESTONES M8 task 7 (F-DI-12): Alt+Space → global Dictate key / ⌥Space mac.
- ARCHITECTURE §12.5 composer dictation trigger rules; new IPC command `dictate_composer_focus {targetId?}` declared in §3.2 Dictate table.
- UI_SPEC per-screen fixes: §3.3 rail collapse Mod+B → Mod+\; UI-10 "Launch several… ⌘T" → ⇧⌘L; UI-11 Tidy Mod+Alt+T → Mod+Shift+T, Maximize Mod+Enter → Mod+Shift+Enter, focus cycle notes Win/Linux alias; UI-22 model picker Mod+Shift+M → Mod+Shift+O (Mod+Shift+M is Conductor mute); UI-70 "Mod+Shift+N new teammate" removed (collides with Notifications).
- UI_SPEC §5: Plan/Act toggle ⇧Tab → Mod+Shift+P (matches UI-22; Shift+Tab must keep reverse focus traversal, WCAG); added thread/composer/teammate rows (Mod+Shift+O, Mod+U, Mod+L, Mod+Alt+C, Mod+F in thread, Alt+↑/↓, Mod+Shift+H, Mod+Alt+1…4, Mod+Backspace) so every per-screen shortcut is in the canonical map.
- ARCHITECTURE §5.9 key-interception list aligned with UI_SPEC §5 / F-SH-09 (adds Ctrl+Alt+…, Ctrl+Tab, F6).
## 4. Worktree prefix
- UI_SPEC §2.24 ApprovalCard wireframe "Branch: tn/fix-auth" → "tandem/fix-auth"; UI_SPEC §4.3 intro removed the "read `tn/…` as `tandem/…`" caveat (no `tn/` remains). All other docs already use `<repo>/.tandem/worktrees/<slug>` on `tandem/<slug>`.
## 5. Approval kinds
- PRODUCT_SPEC F-TH-03 normalized-event table: `kind (command/edit/mcp/network/other)` → full `ApprovalKind` superset (command, file_write, file_delete, network, mcp, plugin_action, spend, question, other; edit = file_write).
- UI_SPEC §2.24 ApprovalCard `kind` prop: added `mcp`, `other`; default-actions table adds a network/mcp/other row; "Allow for this session" → "Allow for this thread" (matches F-TH-11 and UI-23); plugin_action row covers write+publish, checkbox renamed.
- UI_SPEC UI-93: added Publish (`external_publish`) and Delete (`destructive`) rows so all 7 F-PL-06 classes appear; row→class mapping; defaults tied to ARCHITECTURE §11.7; "Let Full access chats run writes without asking" default on (was off, contradicting F-PL-06); added "publish in Full" toggle; plugin card checkbox "Always allow … in this chat" → "Allow … for the rest of this chat" with explicit mapping to F-PL-07 "Allow for this conversation".
## 6. Gateway built-in tool naming
- PRODUCT_SPEC F-PL-01: added reserved plugin id `tandem` rule (built-ins `tandem__<tool>`, e.g. `mcp__tandem__tandem__send_message`; permission tool `tandem_approvals__request`); built-in list renamed to `tandem__session_search`, `tandem__send_message`, `tandem__list_teammates`, `tandem__schedule_{list,create,update,delete}`, `tandem__memory_write`, `tandem__read_skill`, `tandem_approvals__request`.
- PRODUCT_SPEC F-TH-04 `tandem_approvals.request` → `mcp__tandem__tandem_approvals__request`; F-AG-05 AC2, F-AG-06, F-AG-07, F-AG-11, F-AG-12, F-RT-05 (+AC2) use the prefixed names.
- MILESTONES M7 tasks (approvals built-in, Claude flags), M10 (memory_write, read_skill, session_search, send_message/list_teammates, E2E), M16 (schedule_create) use the prefixed names.
## 7. Composer Plan | Act and access labels **(promoted from UI_SPEC §2.23/§4.3)**
- PRODUCT_SPEC F-TH-01 thread fields "mode (Plan/Agent)" → "(Plan/Act)"; F-TH-07 toolbar "Plan or Agent" → "Plan | Act" segmented control with the Agent-mode collision rationale; "Read / Edits / Full" → "Read only / Edits / Full access" (wire values `read`/`edits`/`full`); F-TH-09 table labels; F-TH-10 "switches the thread to Agent mode" → "switches the composer to Act"; F-SH-04 Agents page, F-TH-02 folderless default, F-AG-05 Default access row use the new labels.
- ARCHITECTURE §4.2 `threads.mode` CHECK ('plan','agent') → ('plan','act') with a comment mapping access values to the UI labels.
- MILESTONES E2E-M7-6 "the mode becomes Agent" → "the composer mode becomes Act".
## Env vars, fixtures, seeds (other contradictions)
- MILESTONES §1: `TANDEM_KEYRING=mock` (3 places) → `TANDEM_FAKE_KEYCHAIN=1` (ARCHITECTURE §2.2/§4.4/§17.5); `window.__TANDEM_TEST__` → `window.__tandemTest`; `tandem-fake-engine` behind feature `fixtures` with `FAKE_SCENARIO`/`$FAKE_LOG` → ARCHITECTURE §6.12 `fake-engine` with `TANDEM_FAKE_ENGINE`/`TANDEM_FAKE_SCRIPT`/`TANDEM_FAKE_RECORD`/`TANDEM_FAKE_MODE`; fixture files `*.jsonl` + `*.golden.json` → `<scenario>.script.jsonl` + `<scenario>.expected.json`; M3 mocks line renamed; `TANDEM_API_URL` default `http://127.0.0.1:8787` → `http://api.tandem.local:8787` with the §17.5 127.0.0.1 fallback.
- ARCHITECTURE §17.5 (**(promoted)** from MILESTONES, which the tests depend on): seeds `dev@tandem.local`/`tandem-dev-password` + `pro@tandem.local` → MILESTONES' `free@/pro@/pastdue@/lapsed@/admin@/owner@/member@tandem.test`, password `Tandem-dev-pass-1`; API env adds `AI_PROVIDER`; desktop env list adds the MILESTONES debug-only test variables, the fake-engine variables and `TANDEM_API_TEST_ROUTES`.
## 8. DECISIONS crate/binary list
- DECISIONS monorepo layout: src-tauri now "crate `tandem-desktop`: lib `tandem_app` (services) + bin `tandem`"; tandem-engines lists bins `tandem-hook` (sidecar) and `fake-engine` (test-only); tandem-tasks "binary + lib"; added `tests/invariants/` (`tandem-invariants`, MILESTONES D-10). DECISIONS lists no API route families, so none added.
- ARCHITECTURE §2.1 Cargo `members` adds `tests/invariants` (otherwise `cargo test -p tandem-invariants` in M15 cannot run).
## 9. Build sequence vs MILESTONES
- Verified: MILESTONES §20 coverage table = `**Owns:**` lines = PRODUCT_SPEC headings (152 IDs; 0 missing/duplicate; P2 only in M16) — the §20.1 script passes after all edits.
- PRODUCT_SPEC §7.2: added "Execution order is MILESTONES.md" note (MILESTONES governs build order; summary of D-1…D-10) and the list of features absent from the level table (placed by MILESTONES §20).
## 10. Codex adapter
- PRODUCT_SPEC F-TH-05: "Use newConversation / sendUserTurn / interruptConversation (or current equivalents)" → defers to ARCHITECTURE §6.8.2's dual-protocol `CodexProtocol` (V2 thread/turn API + V1 legacy), selected by version; access bullets use the new labels.
## 11. Business numbers
- Verified consistent: Pro $29/mo · $290/yr; Team $49/seat/mo · $490/seat/yr; 10,000 credits/mo (per seat, pooled for Team); packs 5,000/$5 · 12,000/$10 · 30,000/$20; per-feature credit prices (STT 2/15 s, Enhance 3, Conductor 20/10 s, find 1, Auto 6, teammate draft 10, title 0) across PRODUCT_SPEC §3, ARCHITECTURE §14.4–14.5, UI_SPEC UI-120–124/UI-62/UI-61, MILESTONES M1/M13.
- DECISIONS Business model: Free "4 panes, local Dictate, 1 Teammate" → "4 live panes, 3 workspaces, local Dictate, 1 Teammate besides the built-in Lead" (PRODUCT_SPEC §3.2 + UI-122 gate copy; Lead is auto-created so "1 total" would leave Free with no creatable teammate — **(promoted)**); Pro adds 7-day trial; Team adds $490/seat/yr, min 2 seats, 14-day trial, pooled 10,000/seat; added credit packs and the 1 credit = $0.001 rule.
- UI_SPEC UI-120 Free card "1 teammate" → "1 teammate+Lead"; ARCHITECTURE §14.4 LIMITS comment `teammates: 1 /* besides Lead */`.
## 12. Screen IDs
- Checked every `UI-nn` referenced in PRODUCT_SPEC, ARCHITECTURE, MILESTONES, UI_SPEC and BUILD_PROMPT against the UI_SPEC §8 index (83 ids): none missing; every `#### UI-nn` heading is indexed and its §-section matches (UI-01 lives at §3.2 as indexed). No changes needed.
## Settings pages / presets (other contradictions; UI_SPEC §8.1 items 4–5)
- **(promoted)** PRODUCT_SPEC F-SH-04: added "Rendered navigation" bullet making UI-100's 16-page nav binding with the page mapping (theme + terminal font → Appearance; Voice → Dictate + Conductor; Agents → Teammates; Privacy → Privacy & Telemetry; Account → Account & Plan; About → Updates + Advanced; "Run setup again" in General).
- PRODUCT_SPEC F-SH-08 "re-run from Settings → About" → "Settings › General → Run setup again (UI-101)"; F-DI-02 "Settings → Voice → Shortcut" → "Settings › Dictate (UI-62)"; F-CN-06 "Settings → Voice → Conductor" → "Settings › Conductor (UI-67)"; F-AG-09/F-AG-12 "Settings → Agents" → "Settings › Teammates". MILESTONES M10 global pause → "Settings › Teammates (UI-107)".
- UI_SPEC §3.2 credits pill "opens Settings > Billing" → "Settings › Account & Plan (UI-114)"; UI-05 footer "Settings › Agents" → "Settings › Engines"; §8.1 items 4 and 5 resolved.
- UI_SPEC UI-12 presets "Solo | Pair | Trio | Squad" → PRODUCT_SPEC/ARCHITECTURE/MILESTONES "Solo | Pair | Bench | Fleet" (+Custom); "+ Add agent" → "+ Add seat ▾ (Agent · Shell command · Browser URL)" + How many stepper (F-CD-14); roles "stored in Settings > Presets" (no such page) → edited in the sheet, stored with `presets`.
- UI_SPEC UI-04 'Parakeet · 640 MB' → manifest size (670 MB); §8.1 item 6 resolved; §8.1 preamble now says all items are fixed at source.
## 13. IPC names and endpoints
- Extracted 295 command names and 43 event names from ARCHITECTURE §3.2/§3.3. Checked every backticked `<domain>_<verb>` / `<domain>:<event>` token in MILESTONES, UI_SPEC and PRODUCT_SPEC (MILESTONES uses mostly descriptive names; ~60 snake_case identifiers reviewed, the rest are tables, fields or tools). Mismatches fixed:
  - MILESTONES M2 `settings_get` / `settings_list` → `settings_get_all` / `settings_reset` (+ `settings_set`).
  - MILESTONES M5 `onboarding_state` step machine → `OnboardingState` via `onboarding_get`/`onboarding_set`; "re-runnable from About" → Settings › General.
  - MILESTONES M4 registry path `registry/engines.json` → `registry.json`; fields `auth_status_cmd` → `status_cmd`, `keychain_shared` → `keychain_isolation` (the ARCHITECTURE §6.1 names), "Add these to ARCHITECTURE first" removed since they exist; M6 "success detected via auth_status_cmd" → `status_cmd`.
  - New command `dictate_composer_focus` declared (see §3 above).
- ARCHITECTURE §3.2: declared the MILESTONES test harness as debug-only commands `test_counters`, `test_spawn_log`, `test_clock_advance`, `test_auth_refresh` (CLAUDE.md requires every IPC command to be declared).
- ARCHITECTURE §3.2 shorthand rows expanded to full names: `scratchpad_read/_write/_delete`, `task_attempt_resume/_retry`.
- Multi-window: UI_SPEC had "New window" (app menu, §5 ⌥⌘N) and pane "Move to new window", but F-SH-01 specifies one main window and ARCHITECTURE has no window-creation command. Removed New window (UI_SPEC §3.2, §5, and the row I had copied into F-SH-09); pane menu "Move to new window" → "Move to new tab" (F-CD-12); "mode persists per window" → "across launches".
- UI_SPEC UI-11 Auto submenu "Off / Complete / Harden / Review / Custom…" → F-AU-01 modes "Off / Finish / Harden / Review / Explore / Custom…"; badge example "Harden · 3/10" → "3/15" (Harden ceiling).
- Endpoints: extracted every `/v1/...` path in all files and checked against ARCHITECTURE §14.6. Fixed in MILESTONES M1/M13: `DELETE /v1/account` → `DELETE /v1/me`; `/v1/keys` (not an endpoint) → public keys copied to `resources/keys.json` by `scripts/dev-keys.ts`; `POST /v1/auth/desktop/code` → `POST /v1/auth/desktop/authorize`; `POST /v1/auth/desktop/revoke` + `GET /v1/devices` → `POST /v1/auth/desktop/logout` + `GET/DELETE /v1/me/sessions[/:id]`; E2E-M1-1 `POST /v1/account/password` → `POST /v1/auth/change-password`; `POST /v1/admin/refunds` → `POST /v1/admin/users/:id/refund`. ARCHITECTURE §14.6 better-auth list adds `change-password`. Remaining non-catalogue paths are legitimate (`/api/v1/site-info` web route, local status server `/v1/status`, third-party URLs).
## 14. Clean room
- Grepped every kit file for the banned names in DECISIONS §Identity, plus "orb" and "Chief of Staff" (`reference/` is exempt). The only hits are DECISIONS' own prohibition list and "not an orb" rule, which are kept because they must name what is banned. DECISIONS · Visual identity heading: "(own, not <reference product>'s)" → "(Tandem's own)", which removes the only non-rule mention.
- Generic English words containing the banned substring were renamed so a case-insensitive name grep over the kit is clean: UI_SPEC §1.2 CSS comments ("Theme …" → "Theme mapping", "not …-ed" → "not mapped into Tailwind"); PRODUCT_SPEC F-CD-17 and ARCHITECTURE §1.2 "no IPC …" → "no IPC access"; MILESTONES (5×) and ARCHITECTURE §3.2 "test …" → "test harness".
## Precedence statements
- ARCHITECTURE header "UI_SPEC (ui_part1–4)" → "UI_SPEC.md", notes MILESTONES governs build order. UI_SPEC header precedence now lists ARCHITECTURE and MILESTONES and declares §1.2 tokens and §5 map canonical. MILESTONES header: UI_SPEC means `UI_SPEC.md`; `_drafts/` superseded.
## 15. BUILD_PROMPT and the gate
- All files and anchors BUILD_PROMPT names exist (DECISIONS §"Non-negotiable engineering rules", PRODUCT_SPEC IDs, MILESTONES verification commands, `crates/tandem-engines/tests/fixtures/`, `TANDEM_REAL_ENGINES`).
- Gate unified: DECISIONS rule 1, CLAUDE.md and BUILD_PROMPT §2.3 previously had only the 4-command line while MILESTONES G had 3 lines (adds clippy/fmt and protocol drift). All four now carry the identical 3-line gate G, invoked as root script `pnpm gate`.
- BUILD_PROMPT §1: precedence notes the MILESTONES build-order exception, points to CONSISTENCY_REPORT.md, and says to ignore `_drafts/`. Commit example `M3: F-CD-04 split/snap panes` → `F-CD-02` (split/snap is the layout feature).
- ARCHITECTURE §3.1 `pnpm --filter protocol gen` → `pnpm gen:protocol` (the MILESTONES root script used in G).
## Paths under $DATA and keychain services (other contradictions)
- MILESTONES M2 desktop auth: `$DATA/auth.bin` + keychain service `app.tandem.desktop`/`token-key` → ARCHITECTURE §4.3–4.4 `$DATA/auth/credentials.enc` + service `app.tandem.auth` (`app.tandem.dev.auth` in dev)/`credentials-key` (`app.tandem.desktop` is the bundle id, not a keychain service).
- MILESTONES M11 headless lock `$DATA/run/gui.lock` → `$DATA/run/app.lock` (+ `app.lock.json`).
- MILESTONES M14 ControlPath "under `$DATA/run/ssh/`" → `$DATA/run/ssh-%C`, known hosts `$DATA/ssh/known_hosts` (ARCHITECTURE §13.1).
- ARCHITECTURE §4.3 file tree adds `browser-profile/` (used by MILESTONES M9, F-CD-17 Clear data) and `ssh/known_hosts` (already used in §13.1); §0.3 keychain names add `app.tandem.ssh` and the dev namespace; §17.5 desktop env adds `TANDEM_LAUNCHER_ROOT` (used by MILESTONES M11).
- MILESTONES M13 `audit_log` table → Postgres `audit_events` (ARCHITECTURE §14.2). All other SQLite tables named in PRODUCT_SPEC/MILESTONES exist in ARCHITECTURE §4.2.
- MILESTONES M1 Drizzle task: `desktop_codes` → `desktop_login_codes`; `stripe_events` → `webhook_events`; `devices` and `deletion_requests` (not in the schema) → `refresh_tokens` device columns and `user.deletionRequestedAt`; adds `credit_reservations`; points to ARCHITECTURE §14.2. Env `BREACHED_CHECK` → `BREACH_CHECK` (ARCHITECTURE §17.5).
- UI_SPEC §3.2 title bar was missing three F-SH-01 items (breaking F-SH-01 AC6): added 1a Left rail toggle (Mod+\), 2a Workspace menu (Code only: Tidy panes, Flip terminals, Show terminal sessions, Usage…), 6a Update pill ("Restart to update" / "Update failed — Retry" → UI-130). Usage cluster now shows the F-AC-04 chip text ("Claude · 82% · resets 4:10 PM") plus the 5h/Wk meters at ≥1280px.
- MILESTONES M2 HUMAN_TODO: public keys "bundled at build as `TANDEM_ENTITLEMENT_PUBKEYS`" → bundled into `resources/keys.json` (ARCHITECTURE §14.4).
- ARCHITECTURE §13.1 SSH integration test and §17.3 `ssh-integration` CI job now use MILESTONES M14's compose `sshd` service (profile `remote`, port 2222) and both test entry points.
- UI_SPEC §1.2/§7.3 (found while verifying ratios): light `--tn-text-subtle` #64748b measures 4.34:1 on `--tn-bg`/surface-4 (slate-50), failing AA → #5f6f84 (5.1 on white, 4.7 on slate-50). Verified ratios: teal-700/white 5.47, sky-700 5.93, amber-700 5.02, green-700 5.02, dark #7d8ba0 on surface-1…4 = 5.54/5.37/5.13/4.70.

---

## Decisions made in this pass (log them in BUILD_LOG → Decisions at M0)
1. **Composer dictation key.** `Alt+Space` is not bound on Windows or Linux, because both OSes reserve it for the window menu. Composer dictation uses the global Dictate hold key (Fn / Ctrl+Win / Ctrl+Alt) while a composer has focus. macOS additionally accepts ⌥Space. New IPC command: `dictate_composer_focus`.
2. **Light-theme accent is teal-700 #0F766E.** Dark stays #2DD4BF. Status text uses the new `status-*-text` tokens.
3. **Meter thresholds are 80% (warning) and 95% (danger).** The earlier 70/90 values contradicted the F-AC-04/F-AC-06 acceptance tests.
4. **Free plan = 1 teammate besides Lead** (Lead doesn't count).
5. **Thread mode values** are stored as `plan`/`act`. Access values stay `read`/`edits`/`full`, and the UI labels them "Read only" / "Edits" / "Full access".
6. **One main window.** "New window" and "Move to new window" were removed.
7. **Shortcut changes.** Plan/Act is `Mod+Shift+P` and the model picker is `Mod+Shift+O`. "New teammate" has no default shortcut.
8. **Global gate G** is the 3-line MILESTONES gate everywhere, exposed as `pnpm gate`.

## Remaining known open items (not changed; need a human or build-time decision)
1. **`_drafts/ui_part1–4.md` are stale.** They still contain the old tokens, `tn/` examples, "Trio/Squad" presets and so on. BUILD_PROMPT and MILESTONES now say to ignore them, but deleting the folder is the safer option. It wasn't deleted here.
2. **Clean-room residue in UI copy.** Several labels coincide with the researched product's UI text. They are generic English, but they are the same strings: "Tidy panes", "Flip terminals", "Show terminal sessions", the rail labels "Working / Needs you / Failed", the Auto modes "Harden" and "Review", and the pinned "Lead" hero tile, which plays the reference's "Chief of Staff" role. A human should decide before launch whether to rename them. The internal names Teammate and Routine are fixed by DECISIONS.
3. **App-shortcut rebinding is P2.** No milestone explicitly builds UI-113's per-row "Record…" buttons for app shortcuts. Until M16, UI-113 should show the map read-only; global Dictate rebinding still ships with F-DI-01. Confirm which milestone owns rebinding.
4. **MILESTONES IPC wording is still descriptive.** Only the exact names found were fixed (about 60 identifiers reviewed). Other behaviours described in prose must be mapped to ARCHITECTURE §3 names during implementation.
5. **Codex method names must be verified at build time.** Both the V2 and V1 names are frozen against the installed CLI schema (ARCHITECTURE §6.8.2). The same applies to the engine registry values marked ✓ and the Hugging Face model URLs and hashes.
6. **UI-93 now lets users make a risk class stricter than the F-PL-06 default.** That is an addition beyond PRODUCT_SPEC, and it is harmless to the decision table tests. It isn't yet reflected in F-PL-06 prose.
7. **Light-theme terminal cursor and ANSI cyan still use teal-600 `#0d9488`.** That is acceptable for non-text (≥3:1), but it is off-token relative to the new light accent.
8. **UI_SPEC still says "this part" / "ui_part2" in places.** These are harmless internal references. MILESTONES now explains them.

## Post-review changes (orchestrator)
- Deleted `_drafts/` (stale UI part files).
- Renamed labels that matched the researched product's wording, across PRODUCT_SPEC, UI_SPEC, MILESTONES and ARCHITECTURE:
  - "Tidy panes"/"Tidy" → "Auto-arrange panes"/"Arrange"
  - "Flip terminals"/"Flip" → "Card view"
  - "Needs you"/"Needs your attention" → "Waiting on you"
  - Auto mode "Harden" → "Stress-test"
  Shortcuts are unchanged (Arrange stays `Mod+Shift+T`).
- Shortcut rebinding: confirmed P2 under F-SH-09 → owned by M16 with the other P2 items.
