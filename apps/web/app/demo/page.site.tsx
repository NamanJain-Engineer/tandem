import { buttonClass } from "@tandem/ui/components/Button";
import { Download, Play } from "lucide-react";
import type { Metadata } from "next";
import type { ReactNode } from "react";
import { Icon } from "../../components/site/Icon";
import { Keys } from "../../components/site/Keys";
import { PageHero } from "../../components/site/PageHero";
import { SiteFrame } from "../../components/site/SiteFrame";
import { SITE } from "../../lib/site";

export const metadata: Metadata = {
  title: "Live demo",
  description: `Try ${SITE.product} in your browser: the real interface, with simulated agents.`,
  alternates: { canonical: "/demo/" },
};

/** The demo app the button opens (a static build of the desktop interface). */
export const DEMO_APP = "/demo/app/";

const TRY: ReadonlyArray<{ title: ReactNode; body: string }> = [
  { title: "Send the message that's waiting", body: "The agent reads the code, edits two files and asks before running the tests." },
  { title: "Answer the approval", body: "Allow once, allow for this thread, or deny, right in the conversation." },
  { title: "Open Changes", body: "Review every file the turn touched, with its diff." },
  {
    title: (
      <>
        Press <Keys id="mode.code" /> for Code mode
      </>
    ),
    body: "Agents and a shell in split panes. Type help in the shell.",
  },
  {
    title: (
      <>
        Press <Keys id="palette.open" />
      </>
    ),
    body: "The command palette: every action, searchable.",
  },
];

export default function DemoPage() {
  return (
    <SiteFrame current="/demo/">
      <PageHero
        eyebrow="Runs in your browser"
        title={`Try ${SITE.product} without installing it`}
        lead={
          <>
            This is {SITE.product}&apos;s real interface, built for the browser. The agents and terminals in it are
            simulated: they follow a script, nothing runs, and nothing you type leaves your browser. In the app
            they&apos;re Claude Code, Codex and the other agents you have installed, on your own machine and your own
            plan.
          </>
        }
      >
        <div className="flex flex-wrap items-center gap-3 pt-2">
          <a href={DEMO_APP} data-testid="launch-demo" className={buttonClass("primary", "xl")}>
            <Icon icon={Play} size={18} />
            Launch the demo
          </a>
          <a href="/download/" className={buttonClass("outline", "xl")}>
            <Icon icon={Download} size={18} />
            Download for Linux
          </a>
        </div>
        <p className="text-caption text-fg-muted">Best on a laptop or desktop screen.</p>
      </PageHero>

      <section aria-label="The demo" className="bg-page pb-16">
        <div className="mx-auto w-full max-w-stage px-(--site-gutter)">
          <a href={DEMO_APP} data-stage="" className="group block overflow-hidden rounded-lg bg-stage shadow-stage">
            <picture>
              <source type="image/avif" srcSet="/demo/still.avif" />
              <source type="image/webp" srcSet="/demo/still.webp" />
              <img
                src="/demo/still.jpg"
                width={1280}
                height={800}
                fetchPriority="high"
                decoding="async"
                alt="Launch the demo: Vibezeo's Thread mode with a thread about a redirect loop after login, its first message ready to send."
                className="block h-auto w-full transition-opacity duration-(--dur-base) group-hover:opacity-90"
              />
            </picture>
          </a>
        </div>
      </section>

      <section aria-labelledby="try" className="bg-raised py-(--site-section-y)">
        <div className="container-site flex flex-col gap-10">
          <div className="flex flex-col gap-4">
            <p className="eyebrow">In five steps</p>
            <h2 id="try" className="text-display-lg text-fg">
              What to try
            </h2>
          </div>
          <ol className="grid gap-4 sm:grid-cols-2 lg:grid-cols-5">
            {TRY.map((t, i) => (
              <li key={t.body} className="flex flex-col gap-3 rounded-md border border-border bg-card p-6">
                <span className="font-mono text-caption text-fg-subtle">{String(i + 1).padStart(2, "0")}</span>
                <span className="text-copy font-semibold text-fg">{t.title}</span>
                <span className="text-copy-sm text-fg-muted">{t.body}</span>
              </li>
            ))}
          </ol>
        </div>
      </section>
    </SiteFrame>
  );
}
