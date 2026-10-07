import type { Metadata } from "next";
import type { ReactNode } from "react";
import { SiteFrame } from "../../components/site/SiteFrame";
import { SITE } from "../../lib/site";

export const metadata: Metadata = { title: `About ${SITE.company}` };

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
        <a key="founder" href={SITE.founder.link} className="text-accent hover:underline">
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
      <a key="email" href={`mailto:${SITE.contactEmail}`} className="text-accent hover:underline">
        {SITE.contactEmail}
      </a>,
    ],
    ["Website", SITE.url.replace("https://", "")],
  );
  return rows;
}

export default function AboutPage() {
  return (
    <SiteFrame>
      <section className="mx-auto flex max-w-5xl flex-col gap-5 px-6 pb-10 pt-16">
        <p className="text-xs font-medium uppercase tracking-caps text-accent">About</p>
        <h1 className="max-w-3xl text-3xl font-semibold tracking-tight">
          {SITE.company} builds {SITE.product}, a desktop workspace for running coding agents side by side.
        </h1>
        <p className="max-w-3xl text-lg text-fg-muted">
          {SITE.company} is a small, independent software company
          {SITE.founded && SITE.location ? `, founded in ${SITE.founded} in ${SITE.location}` : ""}, and right
          now it's one person. It started from a simple observation: as coding agents multiply, developers
          need a better way to run, coordinate and keep an eye on them. We think the agents developers already
          pay for deserve a proper home on their own machine: real terminals, one view of every agent, and
          nothing taken off the computer without asking.
        </p>
      </section>

      <Block id="founder" title="Founder">
        <div className="flex max-w-3xl flex-col gap-2 rounded-md border border-border bg-surface-1 p-6">
          <p className="text-lg font-semibold">
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
          {SITE.founder.bio && <p className="mt-2 text-md text-fg">{SITE.founder.bio}</p>}
          <p className="text-md text-fg-muted">
            {SITE.founder.name} started {SITE.company} to build {SITE.product} and does every job in it:
            product, design, code, releases and support. Every email gets a reply from the founder.
          </p>
          {SITE.founder.link && (
            <a href={SITE.founder.link} className="mt-1 text-base font-medium text-accent hover:underline">
              {SITE.founder.link.replace("https://", "")}
            </a>
          )}
        </div>
      </Block>

      <Block id="how-we-work" title="How we work">
        <ul className="grid gap-4 sm:grid-cols-3">
          {WAYS.map((w) => (
            <li key={w.title} className="flex flex-col gap-1.5 border-l-2 border-accent pl-4">
              <h3 className="text-md font-semibold">{w.title}</h3>
              <p className="text-base text-fg-muted">{w.body}</p>
            </li>
          ))}
        </ul>
      </Block>

      <Block id="company" title="Company details">
        <dl className="grid max-w-3xl grid-cols-[minmax(8rem,auto)_1fr] gap-x-6 gap-y-3 text-md">
          {details().map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-fg-muted">{k}</dt>
              <dd className="text-fg">{v}</dd>
            </div>
          ))}
        </dl>
      </Block>

      <Block id="contact" title="Contact">
        <div className="flex max-w-3xl flex-col gap-3 text-md text-fg-muted">
          <p>
            For support, partnerships or anything else, email{" "}
            <a href={`mailto:${SITE.contactEmail}`} className="text-accent hover:underline">
              {SITE.contactEmail}
            </a>
            .
          </p>
          <p>
            See what's shipped so far in the{" "}
            <a href="/changelog/" className="text-accent hover:underline">
              changelog
            </a>
            .
          </p>
          <p>
            Found a security problem? See{" "}
            <a href="/security/" className="text-accent hover:underline">
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
    <section id={id} aria-labelledby={`${id}-title`} className="scroll-mt-16 border-t border-divider">
      <div className="mx-auto flex max-w-5xl flex-col gap-5 px-6 py-12">
        <h2 id={`${id}-title`} className="text-xl font-semibold tracking-tight">
          {title}
        </h2>
        {children}
      </div>
    </section>
  );
}
