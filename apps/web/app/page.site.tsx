import { Badge } from "@tandem/ui/components/Badge";
import { StatusDot, type Status } from "@tandem/ui/components/StatusDot";
import { Mark, SiteFrame } from "../components/site/SiteFrame";
import { ENGINES, SITE } from "../lib/site";

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
    body: "Tandem knows when an agent is working, waiting for you, done or stuck. That status shows on every pane, tab and workspace.",
  },
  {
    title: "One Dashboard",
    body: "Every live agent in one place, across all your workspaces. Approve or deny tool requests right on the card, without hunting for the right pane.",
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

/** Later milestones, in build order. */
const ROADMAP = [
  {
    title: "Lanes, presets and handoff",
    body: "Run one task across several engines, each in its own git worktree, then compare and hand work from one to another.",
  },
  { title: "Accounts and usage", body: "Several subscriptions per engine, with usage and limits in view." },
  {
    title: "Thread mode",
    body: "Read a coding session as a conversation instead of watching a TUI, with inline approvals and a changes panel.",
  },
  { title: "Dictate", body: "Push-to-talk anywhere in the app, with speech recognized on your machine." },
  { title: "Files, git and a browser", body: "Your files, diffs and a preview browser beside the agents." },
  {
    title: "Teammates (Agent mode)",
    body: "Persistent agents with memory, skills and routines that can message each other.",
  },
  {
    title: "Tasks, routines and plugins",
    body: "A local task tracker agents can read and write, scheduled routines, and connected services behind explicit approvals.",
  },
  { title: "Remote", body: "Run agents over SSH and WSL, and check on them from your phone." },
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
    body: "Unattended work never inherits interactive full access, and spending always asks first.",
  },
  { title: "One codebase", body: "macOS, Windows and Linux get the same features on the same day." },
] as const;

const FAQ = [
  {
    q: "Can I use Tandem today?",
    a: "Not yet. Tandem is in active development and isn't publicly available. Email us if you'd like to try an early build.",
  },
  {
    q: "Which platforms will it support?",
    a: "macOS 14 or later, Windows 10 and 11 (x64), and Linux x86_64 (AppImage and .deb). We develop and test on Linux first, and build every platform in CI.",
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
    a: "We'll publish pricing before launch. There will be a free plan for running your own engines side by side.",
  },
] as const;

/** A static picture of a 2×2 workspace (sample data, no live content). */
const SAMPLE: Array<{ engine: string; status: Status; line: string }> = [
  { engine: "Claude Code", status: "working", line: "Refactoring the session store…" },
  { engine: "Codex", status: "waiting", line: "Wants to run: pnpm test" },
  { engine: "Gemini CLI", status: "done", line: "Updated 4 files" },
  { engine: "zsh", status: "idle", line: "~/src/app $" },
];

function Illustration() {
  return (
    <figure className="flex flex-col gap-2">
      <div className="overflow-hidden rounded-lg border border-border bg-surface-1 shadow-3">
        <div className="flex h-9 items-center gap-2 border-b border-divider px-3 text-xs text-fg-muted">
          <Mark size={14} />
          <span className="font-medium text-fg">app</span>
          <span aria-hidden="true">·</span>
          <span>4 panes</span>
        </div>
        <div className="grid grid-cols-2 gap-px bg-divider">
          {SAMPLE.map((p) => (
            <div key={p.engine} className="flex min-h-28 flex-col gap-2 bg-surface-2 p-3">
              <div className="flex items-center gap-2 text-xs">
                <StatusDot status={p.status} />
                <span className="font-medium text-fg">{p.engine}</span>
              </div>
              <p className="font-mono text-xs text-fg-muted">{p.line}</p>
            </div>
          ))}
        </div>
      </div>
      <figcaption className="text-xs text-fg-subtle">Illustration · sample data</figcaption>
    </figure>
  );
}

export default function HomePage() {
  return (
    <SiteFrame>
      <section className="mx-auto grid max-w-5xl items-center gap-12 px-6 py-20 md:grid-cols-[1.1fr_1fr]">
        <div className="flex flex-col items-start gap-5">
          <Badge variant="accent" dot>
            In active development
          </Badge>
          <h1 className="text-3xl font-semibold tracking-tight">{SITE.tagline}</h1>
          <p className="text-lg text-fg-muted">
            Tandem runs Claude Code, Codex, Gemini CLI and other coding-agent CLIs side by side in real
            terminals, on your own subscriptions. One Dashboard shows every agent: what it's doing, and when
            it needs you.
          </p>
          <div className="flex flex-wrap gap-3">
            <a
              href={`mailto:${SITE.contactEmail}?subject=Tandem%20early%20access`}
              className="rounded-sm bg-accent px-4 py-2 text-md font-medium text-accent-fg hover:bg-accent-hover"
            >
              Ask for early access
            </a>
            <a
              href="#features"
              className="rounded-sm border border-border-strong px-4 py-2 text-md font-medium text-fg hover:bg-surface-3"
            >
              See what works today
            </a>
          </div>
        </div>
        <Illustration />
      </section>

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
        <p className="max-w-2xl text-md text-fg-muted">
          Tandem finds the engine CLIs installed on your machine and launches them in panes, each signed in to
          your own account. If an engine is missing, Tandem shows you how to install it.
        </p>
        <ul className="flex flex-wrap gap-2">
          {ENGINES.map((e) => (
            <li key={e} className="rounded-sm border border-border bg-surface-1 px-3 py-1.5 text-base">
              {e}
            </li>
          ))}
        </ul>
        <p className="text-sm text-fg-subtle">
          Product names belong to their owners. Tandem isn't affiliated with or endorsed by any engine vendor.
        </p>
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
            {SITE.product} is built by {SITE.founder.name}, the founder of {SITE.company}. Write to{" "}
            <a href={`mailto:${SITE.contactEmail}`} className="text-accent hover:underline">
              {SITE.contactEmail}
            </a>{" "}
            to talk about the product or try an early build.
          </p>
        </div>
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
        <div className="mx-auto flex max-w-5xl flex-col items-start gap-4 px-6 py-16 sm:flex-row sm:items-center">
          <div className="flex flex-col gap-1">
            <h2 className="text-xl font-semibold tracking-tight">Want to try it early?</h2>
            <p className="text-md text-fg-muted">
              Tell us which engines you use and what you'd run side by side.
            </p>
          </div>
          <a
            href={`mailto:${SITE.contactEmail}?subject=Tandem%20early%20access`}
            className="rounded-sm bg-accent px-4 py-2 text-md font-medium text-accent-fg hover:bg-accent-hover sm:ml-auto"
          >
            {SITE.contactEmail}
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
  children: React.ReactNode;
}) {
  return (
    <section id={id} aria-labelledby={`${id}-title`} className="scroll-mt-16 border-t border-divider">
      <div className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-16">
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
