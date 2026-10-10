import { SITE } from "../../../lib/site";
import { Section } from "../Section";
import { TextLink } from "../TextLink";

/** #team (§6.11): the founder, as a note. No photo until the owner supplies one. */
export function Founder() {
  const initials = SITE.founder.name
    .split(/\s+/)
    .map((w) => w[0])
    .join("");
  return (
    <Section id="team" eyebrow="Team" title="Who's building Vibezeo">
      <figure className="reveal flex max-w-narrow flex-col gap-6 rounded-lg border border-border bg-card p-8 sm:flex-row sm:gap-8">
        <span
          aria-hidden="true"
          className="inline-flex size-14 shrink-0 items-center justify-center rounded-full border border-accent/40 bg-accent-soft font-mono text-copy font-semibold text-accent"
        >
          {initials}
        </span>
        <div className="flex flex-col gap-4">
          <blockquote className="text-lead text-pretty text-fg">
            {SITE.founder.bio} {SITE.founder.name} started {SITE.company} because, as coding agents multiply,
            developers need a better way to run, coordinate and keep an eye on them.
          </blockquote>
          <figcaption className="flex flex-col gap-0.5">
            <span className="text-copy font-semibold text-fg">
              {SITE.founder.link ? (
                <a href={SITE.founder.link} className="underline-offset-4 hover:underline">
                  {SITE.founder.name}
                </a>
              ) : (
                SITE.founder.name
              )}
            </span>
            <span className="text-copy-sm text-fg-muted">
              {SITE.founder.role}, {SITE.company}
            </span>
          </figcaption>
          <TextLink href="/about/" className="self-start">
            More about {SITE.company}
          </TextLink>
        </div>
      </figure>
    </Section>
  );
}
