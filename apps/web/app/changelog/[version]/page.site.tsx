import type { Metadata } from "next";
import { CATEGORIES } from "@tandem/protocol/changelog";
import { InlineMarkdown } from "../../../components/site/InlineMarkdown";
import { SiteFrame } from "../../../components/site/SiteFrame";
import { PLATFORM_LABEL, RELEASES, versionPath } from "../../../lib/changelog";
import { SITE } from "../../../lib/site";

type Params = { params: Promise<{ version: string }> };

/** One page per CHANGELOG.md version (F-WB-03). */
export const dynamicParams = false;

export function generateStaticParams(): Array<{ version: string }> {
  return RELEASES.map((r) => ({ version: r.version }));
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { version } = await params;
  return { title: `${SITE.product} ${version}`, alternates: { canonical: versionPath(version) } };
}

export default async function VersionPage({ params }: Params) {
  const { version } = await params;
  const release = RELEASES.find((r) => r.version === version);
  if (!release) throw new Error(`No CHANGELOG.md section for ${version}`);
  return (
    <SiteFrame>
      <article className="mx-auto flex max-w-3xl flex-col gap-4 px-6 pb-16 pt-16">
        <a href="/changelog/" className="text-sm text-accent hover:underline">
          ← Changelog
        </a>
        <h1 className="text-3xl font-semibold tracking-tight">
          {SITE.product} {release.version}
        </h1>
        {release.summary.split("\n\n").map((p) => (
          <p key={p} className="text-lg text-fg-muted">
            {p}
          </p>
        ))}
        {CATEGORIES.map((c) => {
          const entries = release.entries.filter((e) => e.category === c);
          if (!entries.length) return null;
          return (
            <section key={c} className="flex flex-col gap-3 border-t border-divider pt-6">
              <h2 className="text-xl font-semibold tracking-tight">{c}</h2>
              <ul className="flex flex-col gap-2 text-base text-fg-muted">
                {entries.map((e) => (
                  <li key={e.text} className="ml-5 list-disc">
                    <InlineMarkdown text={e.text} />{" "}
                    <span className="text-sm text-fg-subtle">
                      ({e.platforms.map((p) => PLATFORM_LABEL[p]).join(", ")})
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          );
        })}
      </article>
    </SiteFrame>
  );
}
