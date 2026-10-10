import { Badge } from "@tandem/ui/components/Badge";
import type { Metadata } from "next";
import { SiteFrame } from "../../components/site/SiteFrame";
import { SITE } from "../../lib/site";

export const metadata: Metadata = {
  title: "Live demo",
  description: `Try ${SITE.product} in your browser: the real interface, with simulated agents.`,
  alternates: { canonical: "/demo/" },
};

/** The demo app the button opens (a static build of the desktop interface). */
export const DEMO_APP = "/demo/app/";

const TRY = [
  ["Send the message that's waiting", "The agent reads the code, edits two files and asks before running the tests."],
  ["Answer the approval", "Allow once, allow for this thread, or deny, right in the conversation."],
  ["Open Changes", "Review every file the turn touched, with its diff."],
  ["Press Ctrl+2 for Code mode", "Agents and a shell in split panes. Type help in the shell."],
  ["Press Ctrl+K", "The command palette: every action, searchable."],
] as const;

export default function DemoPage() {
  return (
    <SiteFrame>
      <section className="mx-auto flex max-w-6xl flex-col items-start gap-4 px-6 pb-8 pt-16">
        <Badge variant="accent" dot>
          Runs in your browser
        </Badge>
        <h1 className="text-3xl font-semibold tracking-tight">Try {SITE.product} without installing it</h1>
        <p className="max-w-3xl text-lg text-fg-muted">
          This is {SITE.product}&apos;s real interface, built for the browser. The agents and terminals in it are
          simulated: they follow a script, nothing runs, and nothing you type leaves your browser. In the app
          they&apos;re Claude Code, Codex and the other agents you have installed, on your own machine and your own
          plan.
        </p>
        <div className="flex flex-wrap items-center gap-3">
          <a
            href={DEMO_APP}
            data-testid="launch-demo"
            className="rounded-sm bg-accent px-4 py-2 text-md font-medium text-accent-fg hover:bg-accent-hover"
          >
            Launch the demo →
          </a>
          <a
            href="/download/"
            className="rounded-sm border border-border-strong px-4 py-2 text-md font-medium text-fg hover:bg-surface-3"
          >
            Download for Linux
          </a>
        </div>
        <p className="text-sm text-fg-subtle">Best on a laptop or desktop screen.</p>
      </section>

      <section aria-labelledby="try" className="mx-auto max-w-6xl px-6 pb-16">
        <h2 id="try" className="pb-4 text-xl font-semibold">
          What to try
        </h2>
        <ol className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {TRY.map(([title, body], i) => (
            <li key={title} className="flex flex-col gap-1 rounded-md border border-border bg-surface-1 p-4">
              <span className="text-sm text-fg-subtle">{i + 1}</span>
              <span className="font-medium">{title}</span>
              <span className="text-sm text-fg-muted">{body}</span>
            </li>
          ))}
        </ol>
      </section>
    </SiteFrame>
  );
}
