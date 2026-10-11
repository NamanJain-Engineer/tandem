# Go to market

Ready-to-use plans for positioning, messaging, channels, the launch, community
and pricing experiments. Built on `docs/COMPETITIVE_ANALYSIS.md` (October 2026).

Two rules hold for everything here:
- **Only true, checkable claims.** No invented customers, numbers, quotes,
  dates or reviews, and no feature the shipped build lacks (OPERATING §9,
  the site brief §1.1). Figures come from code (`PROGRESS`, `PLANS`, `LIMITS`).
- **No competitor names in public copy**, on the site, in the app or in posts
  we publish as Vibezeo. Describe the category, never another product.

Steps only the owner can take (accounts, posting, spending) are in
`HUMAN_TODO.md` under "Go to market".

---

## 1. Positioning

**Category:** a desktop workspace for the coding-agent CLIs you already pay for.

**For** developers who use more than one coding agent, or who use one on
Linux, and want to run several at once without giving up their own
subscriptions, their terminal or their privacy.

**Unlike** vendor apps (one vendor's agent only), AI editors (their own agent
and metered models) and most orchestrators (macOS only, or a sign-in first),
**Vibezeo**:
1. runs any of 11 engines' own CLIs, unmodified, signed in with your own
   accounts;
2. works on Linux today, with the same features on every OS as the goal;
3. never resells or meters your coding: we don't sell inference (keep this
   point off the site; it's a positioning note, not public copy);
4. needs no account, and nothing leaves your computer unless you turn it on.

Each point answers a top complaint about the category (analysis §2): lock-in,
Mac-only tools, pricing that keeps changing, forced sign-in and telemetry.

## 2. Messaging

Every line below is true of the 0.5.0 build. Re-check against `CHANGELOG.md`
before each use.

| Use | Message |
|---|---|
| One-liner | Run your coding agents side by side, on your own accounts, on your own machine. |
| Linux audience | A Linux app for coding agents, not a port: terminals, worktrees, threads and dictation, on the .deb today. |
| Multi-vendor | Claude Code, Codex, Gemini CLI and eight more, each in its own pane, with its own sign-in. |
| Pricing honesty | Free runs on your computer with no account. Paid plans pay for hosted extras, never for your coding. |
| Privacy | No account needed, no telemetry unless you opt in, and Vibezeo never touches your engines' sign-in tokens. |
| Authority | Agents ask before they write, post, spend or delete. Runs nobody is watching hold those actions for you. |
| Worktrees (with this branch) | Each agent gets its own worktree, set up the way your repo needs: env files copied, setup run, and nothing from a repo runs until you trust it. |

Words to avoid: "unlimited" (Pro has a 64-pane soft cap), "all platforms"
(Linux only today), "AI-powered" filler, any figure not from `PROGRESS`.

## 3. Channels, in order

1. **Our own surfaces.** The site (owned by the redesign session), the changelog
   and "What's new" in the app. Every release gets a changelog entry with
   platform tags; that's the cheapest and most credible channel we have.
2. **Linux communities, now.** The audience no rival serves well: r/linux,
   r/linuxquestions only when asked, r/archlinux and r/Fedora once those
   packages exist, the GNOME and KDE discourse forums where tools are allowed,
   and Linux-focused newsletters and podcasts. One post per release, with the
   real changelog, never a disguised ad.
3. **Coding-agent communities.** r/ClaudeAI, r/ChatGPTCoding, r/LocalLLaMA (for
   OpenCode/Aider users), the engines' Discords where self-promotion channels
   exist. Lead with a concrete workflow ("three agents on one bug, in three
   worktrees, with their .env copied"), shown in a real screen recording
   labelled with its build.
4. **Hacker News.** One Show HN, timed to the first macOS build (most HN
   readers use a Mac; a Linux-only Show HN wastes the shot). Founder posts, and
   answers every comment for the first 6 hours. Prepare answers to the
   predictable questions (§4.3).
5. **X and YouTube from the founder's account.** Short build-in-public clips of
   real features, labelled with the version. No engagement bait.
6. **Directories.** Product Hunt on the macOS launch day + 7; AlternativeTo and
   similar listings (category: "coding agent workspace").
7. **Not now:** paid ads (no conversion data yet; competitors' ads programs
   didn't save them), affiliate schemes, comparison pages naming rivals.

## 4. Launch plan

### 4.1 Linux release posts (now, per release)
- [ ] Release lands on vibezeo.com (coordinator's batched release).
- [ ] 60–90 s screen recording of the release's headline feature, from the
      shipped build, labelled with the version.
- [ ] One post per channel in §3.2–3.3, each written for that community, with
      the changelog link and the SHA-256 note.
- [ ] Reply to every comment within a day; log bugs in the public tracker (§5).

### 4.2 macOS launch (when signed builds exist)
- T−14: macOS build in the hands of 10–20 testers from the community (§5).
- T−7: site, docs and changelog checked against the build; FAQ answers (§4.3).
- T−0: Show HN (morning US Eastern, a weekday), X thread, Linux/macOS posts.
- T+1…7: daily fix releases if needed; a "what we learned" post at T+7.
- T+7: Product Hunt.

### 4.3 Questions to prepare true answers for
- "Is this allowed by Anthropic / OpenAI terms?" Terminal panes run the
  vendor's own CLI, signed in by the CLI itself; Vibezeo never reads those
  tokens. Thread mode's use of `claude -p` needs the legal check in
  HUMAN_TODO before we answer for it.
- "What do you collect?" Nothing by default; link the transparency page and
  the privacy page.
- "Why pay?" Today you don't need to: Free runs locally. Pro pays for hosted
  extras when accounts open (state which are built and which are live).
- "Open source?" Answer what is true at the time. Don't promise a licence.
- "Windows?" State the status from `REMAINING.md`, never a date.

## 5. Community

- **A public tracker.** The code repository is private; create a public
  `vibezeo/feedback` repository (issues + discussions) and link it from the
  app's Help menu and the site. It turns complaints into visible demand
  signals, as the rivals' trackers do for them.
- **Discord or Matrix?** Start with GitHub Discussions only; add a chat
  server when there are enough people to answer each other.
- **Public roadmap** generated from `docs/REMAINING.md` status, with no dates.
- **Changelog cadence:** every release, with the platform tags.
- **Thank contributors** of bug reports by handle in the changelog, with
  their permission.

## 6. Pricing and packaging experiments

Context (analysis §1 and §4): free local orchestration is table stakes, and
tools that charged for nothing hosted died or pivoted. Rivals that resell
inference keep changing prices. Our paid plans charge for hosted features and
team admin, which avoids that trap.

| # | Experiment | Hypothesis | How to run it | Measure (opt-in or server-side only) |
|---|---|---|---|---|
| P1 | **Lift local caps on Free** (live panes, workspaces) | The 4-pane, 3-workspace cap is the main reason a Linux user picks a free rival; Pro converts on hosted features, not caps | Change `LIMITS` for Free in remote config for new installs; keep Pro's hosted features as the line | Pro trial starts per new account (server-side), before vs after |
| P2 | **Annual-first Pro page** ($290/yr shown first) | Annual lowers churn while plans settle | Pricing page order | Annual share of checkouts |
| P3 | **Team trial without a card** | Teams evaluate longer | Stripe trial setting | Trial → paid |
| P4 | **Credit pack visibility** | Users of Conductor voice and Auto will want packs | Show the pack prices in the app's usage page | Pack purchases per Pro user |

Do not run price tests that show different prices to different people at
the same time; it erodes the trust the positioning relies on.

## 7. Distribution

- **Linux packages:** `.rpm` (Fedora, openSUSE) joins the release (the
  coordinator), Flatpak and Snap sit with the mobile-apps session, and an AUR
  package needs the owner's account (HUMAN_TODO). Rivals' Linux apps skip
  Fedora/Arch or lack Flatpak (analysis §1.3).
- **Package managers:** a Homebrew cask and winget manifest when those builds
  exist.
- **Machine-readable discovery** already exists (F-WB-04); keep `SHA256SUMS`
  next to every file.
