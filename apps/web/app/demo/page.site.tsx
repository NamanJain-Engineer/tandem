import { siteButton } from "@tandem/ui/components/SiteButton";
import { Download, Play } from "lucide-react";
import type { Metadata } from "next";
import type { ReactNode } from "react";
import { Icon } from "../../components/site/Icon";
import { Keys } from "../../components/site/Keys";
import { VideoSection } from "../../components/site/home/VideoSection";
import { PageHero } from "../../components/site/PageHero";
import { SiteFrame } from "../../components/site/SiteFrame";
import { SITE } from "../../lib/site";
import { stagger } from "../../lib/motion";

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
        center
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
        <div className="flex flex-wrap items-center justify-center gap-3 pt-2">
          <a href={DEMO_APP} data-testid="launch-demo" className={siteButton("primary", "lg")}>
            <Icon icon={Play} size={18} />
            Launch the demo
          </a>
          <a href="/download/" className={siteButton("secondary", "lg")}>
            <Icon icon={Download} size={18} />
            Download for Linux
          </a>
        </div>
        <p className="text-caption text-fg-muted">Best on a laptop or desktop screen.</p>
      </PageHero>

      <section aria-label="The demo" className="relative bg-page pb-(--site-section-y)">
        <div className="mx-auto w-full max-w-stage px-(--site-gutter)">
          <div className="frame">
          <a href={DEMO_APP} data-stage="" className="frame-screen group block">
            <picture>
              <source type="image/avif" srcSet="/demo/still-800.avif 800w, /demo/still.avif 1280w, /demo/still@2x.avif 2560w" sizes="(min-width: 1312px) 1268px, 100vw" />
              <source type="image/webp" srcSet="/demo/still-800.webp 800w, /demo/still.webp 1280w, /demo/still@2x.webp 2560w" sizes="(min-width: 1312px) 1268px, 100vw" />
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
        </div>
      </section>

      <section aria-labelledby="try" className="relative bg-raised py-(--site-section-y)">
        <div aria-hidden="true" className="horizon absolute inset-x-0 top-0" />
        <div className="container-site flex flex-col gap-10">
          <div className="flex flex-col gap-4">
            <p className="eyebrow">In five steps</p>
            <h2 id="try" className="text-sheen text-display-lg text-fg">
              What to try
            </h2>
          </div>
          <ol className="grid gap-4 sm:grid-cols-2 lg:grid-cols-5">
            {TRY.map((t, i) => (
              <li key={t.body} className={`reveal ${stagger(i)} surface-raised flex flex-col gap-3 p-6`}>
                <span className="keycap !h-8 !min-w-8 self-start !px-0 !text-[0.75rem] text-fg-muted">{String(i + 1).padStart(2, "0")}</span>
                <span className="text-copy font-semibold text-fg">{t.title}</span>
                <span className="text-copy-sm text-fg-muted">{t.body}</span>
              </li>
            ))}
          </ol>
        </div>
      </section>
      <VideoSection />
    </SiteFrame>
  );
}
