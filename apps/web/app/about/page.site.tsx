import type { Metadata } from "next";
import type { ReactNode } from "react";
import { PageHero } from "../../components/site/PageHero";
import { SiteFrame } from "../../components/site/SiteFrame";
import { SITE } from "../../lib/site";
import { stagger } from "../../lib/motion";

export const metadata: Metadata = { title: `About ${SITE.company}`, alternates: { canonical: "/about/" } };

const WAYS = [
  {
    title: "Honest about where it is",
    body: "This site lists what works today and calls the rest a roadmap. No invented customers, quotes or usage numbers.",
  },
  {
    title: "Your machine, your accounts",
    body: `${SITE.product} runs the agents you already pay for, signed in by themselves. Your code, prompts and tokens stay on your computer.`,
  },
  {
    title: "Built with the agents we support",
    body: `${SITE.product} is written with Claude Code, the engine it integrates most deeply, so we use what we build every day.`,
  },
] as const;

/** Company details, only the rows that are known. */
function details(): Array<[string, ReactNode]> {
  const rows: Array<[string, ReactNode]> = [
    ["Company", SITE.company],
    ["Product", SITE.product],
    [
      "Founder",
      SITE.founder.link ? (
        <a key="founder" href={SITE.founder.link} className="text-fg">
          {SITE.founder.name}
        </a>
      ) : (
        SITE.founder.name
      ),
    ],
  ];
  if (SITE.founded) rows.push(["Founded", SITE.founded]);
  if (SITE.legal) {
    rows.push(["Registered name", SITE.legal.name]);
    if (SITE.legal.registration) rows.push(["Registration", SITE.legal.registration]);
    if (SITE.legal.address) rows.push(["Registered address", SITE.legal.address]);
  }
  if (SITE.location) rows.push(["Based in", SITE.location]);
  rows.push(
    [
      "Email",
      <a key="email" href={`mailto:${SITE.contactEmail}`} className="text-fg">
        {SITE.contactEmail}
      </a>,
    ],
    ["Website", SITE.url.replace("https://", "")],
  );
  return rows;
}

export default function AboutPage() {
  return (
    <SiteFrame current="/about/">
      <PageHero
        eyebrow="About"
        title={`${SITE.product} is a desktop workspace for running coding agents side by side.`}
        lead={
          <>
            {SITE.company} is a small, independent software company
            {SITE.founded && SITE.location ? `, founded in ${SITE.founded} in ${SITE.location}` : ""}, and right now
            it&apos;s one person. It started from a simple observation: as coding agents multiply, developers need a
            better way to run, coordinate and keep an eye on them. We think the agents developers already pay for
            deserve a proper home on their own machine: real terminals, one view of every agent, and nothing taken
            off the computer without asking.
          </>
        }
      />

      <Block id="founder" title="Founder">
        <div className="reveal surface relative isolate flex max-w-narrow flex-col gap-2 overflow-hidden p-8 lg:p-10">
          <div aria-hidden="true" className="bg-lens absolute inset-x-0 top-0 -z-10 h-40 opacity-70" />
          <p className="text-title text-fg">
            {SITE.founder.link ? (
              <a href={SITE.founder.link} className="underline-offset-4 hover:underline">
                {SITE.founder.name}
              </a>
            ) : (
              SITE.founder.name
            )}
          </p>
          <p className="text-copy-sm text-fg-muted">
            {SITE.founder.role}, {SITE.company}
          </p>
          {SITE.founder.bio && <p className="mt-2 text-copy text-fg">{SITE.founder.bio}</p>}
          <p className="text-copy text-fg-muted">
            {SITE.founder.name} started {SITE.company} and does every job in it: product, design, code,
            releases and support. Every email gets a reply from the founder.
          </p>
          {SITE.founder.link && (
            <a href={SITE.founder.link} className="mt-1 self-start text-copy-sm font-medium text-fg">
              {SITE.founder.link.replace("https://", "")}
            </a>
          )}
        </div>
      </Block>

      <Block id="how-we-work" title="How we work">
        <ul className="grid gap-4 sm:grid-cols-3">
          {WAYS.map((w, i) => (
            <li key={w.title} className={`reveal ${stagger(i)} surface flex flex-col gap-2 p-6`}>
              <h3 className="text-title text-fg">{w.title}</h3>
              <p className="text-copy-sm text-fg-muted">{w.body}</p>
            </li>
          ))}
        </ul>
      </Block>

      <Block id="company" title="Company details">
        <dl className="surface grid max-w-narrow grid-cols-[minmax(6rem,auto)_minmax(0,1fr)] gap-x-8 gap-y-4 p-6 text-copy lg:p-8">
          {details().map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-fg-muted">{k}</dt>
              <dd className="min-w-0 break-words text-fg">{v}</dd>
            </div>
          ))}
        </dl>
      </Block>

      <Block id="contact" title="Contact">
        <div className="flex max-w-narrow flex-col gap-3 text-copy text-fg-muted">
          <p>
            For support, partnerships or anything else, email{" "}
            <a href={`mailto:${SITE.contactEmail}`} className="text-fg">
              {SITE.contactEmail}
            </a>
            .
          </p>
          <p>
            See what's shipped so far in the{" "}
            <a href="/changelog/" className="text-fg">
              changelog
            </a>
            .
          </p>
          <p>
            Found a security problem? See{" "}
            <a href="/security/" className="text-fg">
              how to report it
            </a>
            .
          </p>
        </div>
      </Block>
    </SiteFrame>
  );
}

function Block({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section id={id} aria-labelledby={`${id}-title`} className="relative scroll-mt-(--site-header-h) bg-page">
      <div aria-hidden="true" className="horizon absolute inset-x-0 top-0" />
      <div className="container-site flex flex-col gap-8 py-20 lg:py-24">
        <h2 id={`${id}-title`} className="text-sheen text-display-lg text-fg">
          {title}
        </h2>
        {children}
      </div>
    </section>
  );
}
