import { Badge } from "@tandem/ui/components/Badge";
import type { Metadata } from "next";
import { PageHero } from "../../components/site/PageHero";
import { Section } from "../../components/site/Section";
import { SiteFrame } from "../../components/site/SiteFrame";
import { TextLink } from "../../components/site/TextLink";
import { cx } from "../../lib/cx";
import { PLATFORM_LABEL, RELEASES, versionPath } from "../../lib/changelog";
import { MILESTONES, PROGRESS } from "../../lib/progress";
import { RELEASE } from "../../lib/release";
import { SITE } from "../../lib/site";

export const metadata: Metadata = { title: "Changelog", alternates: { canonical: "/changelog/" } };

/** A release's Added lead phrases ("**Dictate.** …" → "Dictate"). */
const highlights = (version: string) =>
  (RELEASES.find((r) => r.version === version)?.entries ?? [])
    .filter((e) => e.category === "Added")
    .flatMap((e) => {
      const m = /^\*\*(.+?)\.?\*\*/.exec(e.text);
      return m?.[1] ? [m[1]] : [];
    });

export default function ChangelogPage() {
  return (
    <SiteFrame current="/changelog/">
      <PageHero
        eyebrow="Changelog"
        title={`What's shipped in ${SITE.product} so far`}
        lead={
          <>
            {SITE.product} is built in milestones. Each one closes with its features tested end to end on the real
            app. The code this release is built from has {PROGRESS.automatedTests.toLocaleString("en-US")} automated
            tests and {PROGRESS.endToEndSuites.toLocaleString("en-US")} end-to-end suites. The latest release,{" "}
            <a href={versionPath(RELEASE.version)} className="text-accent">
              {SITE.product} {RELEASE.version} for Linux
            </a>
            , is out:{" "}
            <a href="/download/" className="text-accent">
              download it
            </a>
            .
          </>
        }
      />

      <section aria-labelledby="releases-title" className="bg-page pb-(--site-section-y)">
        <div className="container-site flex flex-col gap-10">
          <h2 id="releases-title" className="text-sheen text-display-lg text-fg">
            Releases
          </h2>
          <ol className="flex flex-col">
            {RELEASES.map((r, i) => {
              const platforms = [...new Set(r.entries.flatMap((e) => e.platforms))];
              const added = highlights(r.version);
              return (
                <li key={r.version} className="relative grid gap-4 pb-12 pl-8 lg:grid-cols-12 lg:gap-6 lg:pl-0">
                  <span aria-hidden="true" className="absolute bottom-0 left-[5px] top-2 w-px bg-[linear-gradient(180deg,var(--site-line-strong),var(--site-line))] lg:left-[calc(25%-1.25rem)]" />
                  <span
                    aria-hidden="true"
                    className={cx(
                      "absolute left-0 top-2 size-[11px] rounded-full border-2 border-page lg:left-[calc(25%-1.25rem-5px)]",
                      i === 0 ? "bg-accent shadow-[0_0_0_4px_var(--tn-accent-soft),0_0_16px_var(--tn-accent)]" : "bg-line-strong",
                    )}
                  />
                  <div className="flex flex-col items-start gap-2 lg:col-span-3 lg:sticky lg:top-24 lg:self-start">
                    <a href={versionPath(r.version)} className="font-mono text-display-md tracking-[-0.04em] text-fg transition-colors duration-(--dur-base) hover:text-accent">
                      {SITE.product} {r.version}
                    </a>
                    <div className="flex flex-wrap gap-1.5">
                      {i === 0 && (
                        <Badge variant="accent" dot>
                          Latest
                        </Badge>
                      )}
                      {platforms.map((p) => (
                        <Badge key={p} variant="outline">
                          {PLATFORM_LABEL[p]}
                        </Badge>
                      ))}
                    </div>
                  </div>
                  <div className="reveal surface flex flex-col gap-4 p-6 lg:col-span-8 lg:col-start-5 lg:p-7">
                    {r.summary && <p className="text-copy text-pretty text-fg-muted">{r.summary.split("\n\n")[0]}</p>}
                    {added.length > 0 && (
                      <ul aria-label="Added" className="flex flex-wrap gap-2">
                        {added.map((a) => (
                          <li key={a} className="rounded-full bg-(--site-glass) px-3 py-1 text-caption text-fg shadow-[inset_0_0_0_1px_var(--site-line-strong)]">
                            {a}
                          </li>
                        ))}
                      </ul>
                    )}
                    <TextLink href={versionPath(r.version)} className="self-start">
                      Release notes<span className="sr-only"> for {r.version}</span>
                    </TextLink>
                  </div>
                </li>
              );
            })}
          </ol>
        </div>
      </section>

      <Section id="milestones" tone="raised" eyebrow="Milestones" title="How it was built">
        <ol className="grid gap-4 md:grid-cols-2">
          {MILESTONES.map((m) => (
            <li
              key={m.id}
              id={m.id.toLowerCase()}
              className="reveal surface-raised flex scroll-mt-(--site-header-h) flex-col gap-4 p-6 lg:p-7"
            >
              <div className="flex flex-col gap-1">
                <span className="font-mono text-[0.75rem] uppercase tracking-[0.12em] text-accent">{m.id}</span>
                <h3 className="text-title text-fg">{m.title}</h3>
              </div>
              <ul className="flex flex-col gap-2 text-copy-sm text-fg-muted">
                {m.items.map((item) => (
                  <li key={item} className="ml-4 list-disc pl-1 marker:text-fg-subtle">
                    {item}
                  </li>
                ))}
              </ul>
            </li>
          ))}
        </ol>
      </Section>
    </SiteFrame>
  );
}
