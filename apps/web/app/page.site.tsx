import { Badge } from "@tandem/ui/components/Badge";
import type { ReactNode } from "react";
import { SiteFrame } from "../components/site/SiteFrame";
import { PLANS } from "../lib/pricing";
import { PROGRESS } from "../lib/progress";
import { RELEASE } from "../lib/release";
import { EARLY_ACCESS_HREF, ENGINES, SITE } from "../lib/site";

/** What the desktop app does today (milestones M0–M5). */
const FEATURES = [
  {
    title: "Real terminals, side by side",
    body: "Each agent runs in a real PTY pane over your own folders. Split, maximize and rearrange panes into layouts. Full-screen TUIs, colours and mouse input behave as they do in your usual terminal.",
  },
  {
    title: "Your accounts, untouched",
    body: "Each engine signs in by itself, inside its own provider home on your disk. Tandem never reads, copies or proxies an AI vendor's tokens.",
  },
  {
    title: "Live status for every agent",
    body: "Tandem knows when an agent is working, waiting for you, done or has failed, from Claude Code's hooks and from each engine's own output. The status shows on every pane and in the sidebar.",
  },
  {
    title: "One Dashboard",
    body: "Every live agent in one place, across all your workspaces: what each one is doing and which ones are waiting for you. One click jumps to the pane that needs you.",
  },
  {
    title: "Notifications that matter",
    body: "You get an OS notification when an agent needs you or finishes, plus an in-app history. Nothing pings you for the pane you're already looking at.",
  },
  {
    title: "Crash-safe restore",
    body: "Layouts, tabs and panes come back after a quit or a crash. Agents return paused, ready to resume their session, and nothing re-runs on its own. We test this by force-killing the app at random points.",
  },
  {
    title: "Signed updates",
    body: "Updates are signature-checked before they install. An older or tampered build is refused.",
  },
  {
    title: "Private by default",
    body: "Usage statistics and crash reports are off until you turn them on. Secrets live in your OS keychain, and logs are scrubbed of tokens and keys.",
  },
] as const;

/** How Tandem builds on Claude: what's shipped, how it's built, and what's built but not live yet. */
const CLAUDE = [
  {
    tag: "Shipped",
    title: "Claude Code, integrated first",
    body: "Claude Code is the engine Tandem integrates most deeply. Tandem installs Claude Code's own hooks in a separate home for each account, so it knows exactly when a session starts, runs a tool, needs you or finishes. After a restart it reopens the same Claude Code session with --resume.",
  },
  {
    tag: "How we build",
    title: "Built with Claude Code",
    body: "Tandem is built with Claude Code. Most of the code, including the desktop app, the backend, this site and their tests, was written with it, and the commit history records it.",
  },
  {
    tag: "Built, live when accounts open",
    title: "Hosted features on the Claude API",
    body: "Tandem's hosted AI runs on the Claude API. Session titles and Enhance and Polish for dictated prompts are built: the backend calls Claude, meters credits, and stores no prompts or responses. They go live when accounts open, alongside Thread mode and Dictate in the app. Teammate drafts and the Auto supervisor come next.",
  },
] as const;

/** Later milestones, in build order. */
const ROADMAP = [
  {
    title: "Lanes, presets and handoff",
    body: "Run one task across several engines, each in its own git worktree, then compare and hand work from one to another.",
  },
  {
    title: "Accounts and usage",
    body: "Keep work and personal accounts for each engine apart, each signed in by itself, with usage and limits in view.",
  },
  {
    title: "Thread mode",
    body: "Read a coding session as a conversation instead of watching a TUI, with inline approvals and a changes panel.",
  },
  {
    title: "Dictate",
    body: "Push-to-talk anywhere in the app, with speech recognized on your machine.",
  },
  {
    title: "Files, git and a browser",
    body: "Your files, diffs and a preview browser beside the agents.",
  },
  {
    title: "Teammates (Agent mode)",
    body: "Persistent agents with memory, skills and routines that can message each other.",
  },
  {
    title: "Tasks, routines and plugins",
    body: "A local task tracker agents can read and write, scheduled routines, and connected services behind explicit approvals.",
  },
  {
    title: "Remote",
    body: "Run agents over SSH and WSL, and check on them from your phone.",
  },
] as const;

const PRINCIPLES = [
  {
    title: "Your machine, your accounts",
    body: "Nothing leaves your machine unless you opt in, and engine tokens are never touched.",
  },
  {
    title: "Reliability first",
    body: "Correct terminal behaviour, restore after a crash, and no silent failures.",
  },
  {
    title: "Explicit authority",
    body: "Tandem never answers an agent's request on your behalf, and spending always asks first.",
  },
  {
    title: "One codebase",
    body: "macOS, Windows and Linux get the same features on the same day.",
  },
] as const;

const FAQ = [
  {
    q: "Can I use Tandem today?",
    a: "Yes. Tandem 0.1.0 is released for Linux: download it from the download page. macOS and Windows are next. It runs the Free plan on your computer with no account, and doesn't contact our servers. Accounts, Pro and Team come later.",
  },
  {
    q: "How does Tandem use Claude?",
    a: "Claude Code is the engine Tandem integrates most deeply, and Tandem itself is built with Claude Code. Its hosted AI features run on the Claude API: session titles and Enhance and Polish are built, and go live when accounts open.",
  },
  {
    q: "Which platforms does it support?",
    a: "Linux x86-64 today (.deb for Debian and Ubuntu). macOS and Windows 10 and 11 are next: the app already compiles for both in CI. We develop and test on Linux first.",
  },
  {
    q: "Does Tandem see my API keys or subscription tokens?",
    a: "No. Each engine CLI signs in by itself, in its own folder on your disk. Tandem starts the CLI and shows its terminal, and never reads, copies or forwards the engine's credentials.",
  },
  {
    q: "Do I need a new AI subscription?",
    a: "No. Tandem runs the coding agents you already have, on your existing plans.",
  },
  {
    q: "What will it cost?",
    a: "Free is $0 and available now. Pro ($29 a month) and Team ($49 per seat a month) aren't on sale yet. The plans are on the Pricing page.",
  },
] as const;

/** A screenshot of the real app, captured by `apps/desktop/e2e/screens/site.screens.ts`. */
function Screenshot({
  src,
  alt,
  caption,
  width,
  height,
}: {
  src: string;
  alt: string;
  caption: string;
  width: number;
  height: number;
}) {
  return (
    <figure className="flex flex-col gap-2">
      <a href={src} title="Open full size">
        <img
          src={src}
          alt={alt}
          width={width}
          height={height}
          // Sized from width/height before it loads, so the page doesn't shift
          // under a #demo link; never wider than the capture itself.
          style={{ maxWidth: width }}
          className="h-auto w-full rounded-lg border border-border shadow-3"
        />
      </a>
      <figcaption className="text-xs text-fg-subtle">{caption}</figcaption>
    </figure>
  );
}

const STATS = [
  { value: String(PROGRESS.milestones), label: "milestones shipped" },
  {
    value: PROGRESS.automatedTests.toLocaleString("en-US"),
    label: "automated tests",
  },
  {
    value: String(PROGRESS.endToEndSuites),
    label: "end-to-end suites on the real app",
  },
  { value: String(PROGRESS.engines), label: "coding agents supported" },
] as const;

export default function HomePage() {
  return (
    <SiteFrame>
      <section className="mx-auto grid max-w-6xl items-center gap-12 px-6 py-20 lg:grid-cols-[0.9fr_1.1fr]">
        <div className="flex flex-col items-start gap-5">
          <Badge variant="accent" dot>
            Released · {RELEASE.version} for Linux
          </Badge>
          <h1 className="text-3xl font-semibold tracking-tight">{SITE.tagline}</h1>
          <p className="text-lg text-fg-muted">
            Tandem runs Claude Code, Codex, Gemini CLI and other coding-agent CLIs side by side in real
            terminals, on your own subscriptions. One Dashboard shows every agent: what it's doing, and when
            it needs you.
          </p>
          <div className="flex flex-wrap gap-3">
            <a
              href="/download/"
              className="rounded-sm bg-accent px-4 py-2 text-md font-medium text-accent-fg hover:bg-accent-hover"
            >
              Download for Linux →
            </a>
            <a
              href="#demo"
              className="rounded-sm border border-border-strong px-4 py-2 text-md font-medium text-fg hover:bg-surface-3"
            >
              Watch it run (50 s)
            </a>
          </div>
          <p className="text-sm text-fg-muted">
            Free, and out now for Linux. macOS and Windows are next. No account needed: it runs on your
            computer.
          </p>
        </div>
        <Screenshot
          src="/screens/code-mode.png"
          width={1440}
          height={900}
          alt="The Tandem desktop app with four terminal panes over its own repository: the git history, htop, vim open on the Claude Code hook handler, and a passing cargo test run."
          caption="The Tandem desktop app on Linux, four real terminals over its own repository: git history, htop, the Claude Code hook handler in vim, and a test run."
        />
      </section>

      <section aria-label="Progress" className="border-t border-divider">
        <div className="mx-auto flex max-w-6xl flex-col gap-6 px-6 py-10">
          <dl className="grid grid-cols-2 gap-6 sm:grid-cols-4">
            {STATS.map((s) => (
              <div key={s.label} className="flex flex-col gap-1">
                <dt className="order-2 text-sm text-fg-muted">{s.label}</dt>
                <dd className="order-1 text-2xl font-semibold tracking-tight">{s.value}</dd>
              </div>
            ))}
          </dl>
          <p className="text-sm text-fg-muted">
            Built so far, as of {PROGRESS.asOf}.{" "}
            <a href="/changelog/" className="text-accent hover:underline">
              See the changelog →
            </a>
          </p>
        </div>
      </section>

      <Section id="demo" eyebrow="See it run" title="50 seconds, unedited">
        <figure className="flex flex-col gap-3">
          <video
            poster="/screens/code-mode.png"
            width={1280}
            height={800}
            controls
            muted
            playsInline
            preload="metadata"
            style={{ aspectRatio: "1280 / 800" }}
            className="h-auto w-full max-w-full rounded-lg border border-border bg-surface-1 shadow-3"
          >
            {/* VP9 first: open-source Chromium builds can't play H.264. */}
            <source src="/screens/demo.webm" type="video/webm" />
            <source src="/screens/demo.mp4" type="video/mp4" />
          </video>
          <figcaption className="text-sm text-fg-muted">
            A screen recording of the Linux build, with no cuts. 0:00 A new tab finds Claude Code on the
            machine. 0:04 Four terminals over Tandem's own repository: git history, htop, the Claude Code hook
            handler in vim, and the test suite. 0:39 The app is killed with kill -9. 0:42 Relaunched, the
            layout and the four live shells come back, and nothing re-runs on its own.
          </figcaption>
        </figure>
      </Section>

      <Section id="claude" eyebrow="Built with Claude" title="Made for Claude Code, open to every agent">
        <p className="max-w-3xl text-md text-fg-muted">
          Tandem gives Claude Code a persistent workspace: run several sessions at once, see which one needs
          you, and pick up where you left off after a restart, all on your own Claude subscription.
        </p>
        <ul className="grid gap-4 md:grid-cols-3">
          {CLAUDE.map((c) => (
            <li
              key={c.title}
              className="flex flex-col gap-2 rounded-md border border-border bg-surface-1 p-5"
            >
              <span className="text-xs font-medium uppercase tracking-caps text-accent">{c.tag}</span>
              <h3 className="text-md font-semibold">{c.title}</h3>
              <p className="text-base text-fg-muted">{c.body}</p>
            </li>
          ))}
        </ul>
      </Section>

      <Section id="features" eyebrow="Working today" title="Built for running many agents at once">
        <ul className="grid gap-4 sm:grid-cols-2">
          {FEATURES.map((f) => (
            <li
              key={f.title}
              className="flex flex-col gap-2 rounded-md border border-border bg-surface-1 p-5"
            >
              <h3 className="text-md font-semibold">{f.title}</h3>
              <p className="text-base text-fg-muted">{f.body}</p>
            </li>
          ))}
        </ul>
      </Section>

      <Section id="engines" eyebrow="Engines" title="Bring the agents you already use">
        <div className="grid items-center gap-8 lg:grid-cols-[1.2fr_1fr]">
          <div className="flex flex-col gap-5">
            <p className="text-md text-fg-muted">
              Tandem finds the engine CLIs installed on your machine and launches them in panes, each signed
              in to your own account. If an engine is missing, Tandem shows you how to install it.
            </p>
            <ul className="flex flex-wrap gap-2">
              {ENGINES.map((e) => (
                <li key={e} className="rounded-sm border border-border bg-surface-1 px-3 py-1.5 text-base">
                  {e}
                </li>
              ))}
            </ul>
            <p className="text-sm text-fg-subtle">
              Product names belong to their owners. Tandem isn't affiliated with or endorsed by any engine
              vendor.
            </p>
          </div>
          <Screenshot
            src="/screens/launch-grid.png"
            width={544}
            height={570}
            alt="Tandem's launch grid in a new tab: Claude Code 2.1.292 detected, a terminal, and ten more engines with Install buttons."
            caption="A new tab: Tandem found Claude Code on this machine and offers to install the ten engines that aren't."
          />
        </div>
      </Section>

      <Section id="principles" eyebrow="Principles" title="What we won't compromise on">
        <ul className="grid gap-4 sm:grid-cols-2">
          {PRINCIPLES.map((p) => (
            <li key={p.title} className="flex flex-col gap-1.5 border-l-2 border-accent pl-4">
              <h3 className="text-md font-semibold">{p.title}</h3>
              <p className="text-base text-fg-muted">{p.body}</p>
            </li>
          ))}
        </ul>
      </Section>

      <Section id="roadmap" eyebrow="Roadmap" title="What we're building next">
        <ol className="grid gap-x-8 gap-y-5 sm:grid-cols-2">
          {ROADMAP.map((r, i) => (
            <li key={r.title} className="flex gap-3">
              <span className="mt-0.5 font-mono text-sm text-fg-subtle">
                {String(i + 1).padStart(2, "0")}
              </span>
              <div className="flex flex-col gap-1">
                <h3 className="text-md font-semibold">{r.title}</h3>
                <p className="text-base text-fg-muted">{r.body}</p>
              </div>
            </li>
          ))}
        </ol>
      </Section>

      <Section id="team" eyebrow="Team" title="Who's building Tandem">
        <div className="flex max-w-2xl flex-col gap-1 rounded-md border border-border bg-surface-1 p-5">
          <p className="text-md font-semibold">
            {SITE.founder.link ? (
              <a href={SITE.founder.link} className="underline-offset-4 hover:underline">
                {SITE.founder.name}
              </a>
            ) : (
              SITE.founder.name
            )}
          </p>
          <p className="text-base text-fg-muted">
            {SITE.founder.role}, {SITE.company}
          </p>
          <p className="mt-2 text-base text-fg-muted">
            {SITE.founder.bio} {SITE.founder.name} started {SITE.company} to build {SITE.product}: as coding
            agents multiply, developers need a better way to run, coordinate and keep an eye on them.
          </p>
          <a href="/about/" className="mt-2 text-base font-medium text-accent hover:underline">
            More about {SITE.company} →
          </a>
        </div>
      </Section>

      <Section id="pricing" eyebrow="Pricing" title="Free to run your agents">
        <ul className="grid gap-4 sm:grid-cols-3">
          {PLANS.map((p) => (
            <li key={p.id} className="flex flex-col gap-1 rounded-md border border-border bg-surface-1 p-5">
              <h3 className="text-md font-semibold">{p.name}</h3>
              <p className="flex flex-wrap items-baseline gap-x-2">
                <span className="text-2xl font-semibold tracking-tight">{p.price}</span>
                <span className="text-sm text-fg-muted">{p.cadence}</span>
              </p>
              <p className="text-base text-fg-muted">{p.summary}</p>
            </li>
          ))}
        </ul>
        <p className="text-sm text-fg-muted">
          Nothing is on sale yet. Your agents always run on your own subscriptions.{" "}
          <a href="/pricing/" className="text-accent hover:underline">
            Compare the plans →
          </a>
        </p>
      </Section>

      <Section id="faq" eyebrow="FAQ" title="Questions">
        <dl className="flex max-w-3xl flex-col divide-y divide-divider">
          {FAQ.map((f) => (
            <div key={f.q} className="flex flex-col gap-1.5 py-4">
              <dt className="text-md font-semibold">{f.q}</dt>
              <dd className="text-base text-fg-muted">{f.a}</dd>
            </div>
          ))}
        </dl>
      </Section>

      <section className="border-t border-divider">
        <div className="mx-auto flex max-w-6xl flex-col items-start gap-4 px-6 py-16 sm:flex-row sm:items-center">
          <div className="flex flex-col gap-1">
            <h2 className="text-xl font-semibold tracking-tight">Try it on your own projects</h2>
            <p className="text-md text-fg-muted">
              Download {SITE.product} for Linux, then{" "}
              <a href={EARLY_ACCESS_HREF} className="text-accent hover:underline">
                tell us what works and what doesn't
              </a>{" "}
              at {SITE.contactEmail}.
            </p>
          </div>
          <a
            href="/download/"
            className="rounded-sm bg-accent px-4 py-2 text-md font-medium text-accent-fg hover:bg-accent-hover sm:ml-auto"
          >
            Download →
          </a>
        </div>
      </section>
    </SiteFrame>
  );
}

function Section({
  id,
  eyebrow,
  title,
  children,
}: {
  id: string;
  eyebrow: string;
  title: string;
  children: ReactNode;
}) {
  return (
    <section id={id} aria-labelledby={`${id}-title`} className="scroll-mt-16 border-t border-divider">
      <div className="mx-auto flex max-w-6xl flex-col gap-6 px-6 py-16">
        <header className="flex flex-col gap-2">
          <p className="text-xs font-medium uppercase tracking-caps text-accent">{eyebrow}</p>
          <h2 id={`${id}-title`} className="text-2xl font-semibold tracking-tight">
            {title}
          </h2>
        </header>
        {children}
      </div>
    </section>
  );
}
