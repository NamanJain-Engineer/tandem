import { siteButton } from "@tandem/ui/components/SiteButton";
import { Badge } from "@tandem/ui/components/Badge";
import { CATEGORIES } from "@tandem/protocol/changelog";
import { ArrowLeft, ArrowRight, Download } from "lucide-react";
import type { Metadata } from "next";
// The Land scene's styles (D-1880, D-1885).
import "../../../components/site/scenes/scenes.css";
import { Scene } from "../../../components/site/scenes/Scene";
import { LAND_CAPTION, LandScene, landSceneText } from "../../../components/site/scenes/LandScene";
import { Icon } from "../../../components/site/Icon";
import { InlineMarkdown } from "../../../components/site/InlineMarkdown";
import { SiteFrame } from "../../../components/site/SiteFrame";
import { PLATFORM_LABEL, RELEASES, versionPath } from "../../../lib/changelog";
import { RELEASE } from "../../../lib/release";
import { LANDS } from "../../../lib/scenes";
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
  const at = RELEASES.findIndex((r) => r.version === version);
  const release = RELEASES[at];
  if (!release) throw new Error(`No CHANGELOG.md section for ${version}`);
  const newer = RELEASES[at - 1];
  const older = RELEASES[at + 1];
  const platforms = [...new Set(release.entries.flatMap((e) => e.platforms))];
  const latest = release.version === RELEASE.version;
  // 0.9.0 brought Land worktrees: its page plays the scene the home page
  // gave up for length (D-1897, D-1885).
  const scene = release.version === "0.9.0" && LANDS;
  return (
    <SiteFrame current="/changelog/">
      <article className="relative isolate">
      <div aria-hidden="true" className="bg-lens absolute inset-x-0 top-0 -z-10 h-[32rem]" />
      <div className="container-site grid gap-10 py-16 lg:grid-cols-12 lg:gap-6 lg:py-24">
        <aside className="flex flex-col items-start gap-5 lg:sticky lg:top-24 lg:col-span-3 lg:self-start">
          <a href="/changelog/" className="inline-flex min-h-8 items-center gap-1.5 text-copy-sm text-fg-muted">
            <Icon icon={ArrowLeft} size={14} />
            Changelog
          </a>
          <div className="flex flex-wrap gap-1.5">
            {latest && (
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
          {latest && (
            <a href="/download/" className={siteButton("primary", "md")}>
              <Icon icon={Download} size={16} />
              Download {release.version}
            </a>
          )}
          <nav aria-label="Other versions" className="flex flex-col gap-1 text-copy-sm">
            {newer && (
              <a href={versionPath(newer.version)} className="inline-flex min-h-8 items-center gap-1.5 text-fg-muted hover:text-fg">
                Newer: <span className="font-mono">{newer.version}</span>
                <Icon icon={ArrowRight} size={14} />
              </a>
            )}
            {older && (
              <a href={versionPath(older.version)} className="inline-flex min-h-8 items-center gap-1.5 text-fg-muted hover:text-fg">
                <Icon icon={ArrowLeft} size={14} />
                Older: <span className="font-mono">{older.version}</span>
              </a>
            )}
          </nav>
        </aside>
        <div className="flex min-w-0 flex-col gap-6 lg:col-span-8 lg:col-start-5">
          <h1 className="text-sheen font-mono text-display-xl tracking-[-0.05em] text-fg">
            {SITE.product} {release.version}
          </h1>
          {release.summary.split("\n\n").map((p) => (
            <p key={p} className="text-lead text-pretty text-fg-muted">
              {p}
            </p>
          ))}
          {scene && (
            <Scene id="land" text={landSceneText()} caption={LAND_CAPTION} className="rise mt-4">
              <LandScene />
            </Scene>
          )}
          {CATEGORIES.map((c) => {
            const entries = release.entries.filter((e) => e.category === c);
            if (!entries.length) return null;
            const cards = c === "Added";
            return (
              <section key={c} className="flex flex-col gap-5 border-t border-line pt-10">
                <h2 className="eyebrow">{c}</h2>
                <ul className={cards ? "grid gap-4" : "flex flex-col gap-3"}>
                  {entries.map((e) => (
                    <li
                      key={e.text}
                      className={
                        cards
                          ? "reveal surface flex flex-col gap-3 p-6 text-copy-sm leading-7 lg:p-7 text-fg-muted [&>p>strong:first-child]:mb-1 [&>p>strong:first-child]:block [&>p>strong:first-child]:text-title [&>p>strong:first-child]:text-fg [&_code]:font-mono [&_code]:text-[0.875em] [&_code]:text-fg"
                          : "ml-5 list-disc pl-1 text-copy-sm leading-7 text-fg-muted marker:text-fg-subtle [&_code]:font-mono [&_code]:text-[0.875em] [&_code]:text-fg [&_strong]:text-fg"
                      }
                    >
                      <p>
                        <InlineMarkdown text={e.text} />
                      </p>
                      <span className="font-mono text-eyebrow uppercase tracking-[0.08em] text-fg-subtle">
                        {e.platforms.map((p) => PLATFORM_LABEL[p]).join(", ")}
                      </span>
                    </li>
                  ))}
                </ul>
              </section>
            );
          })}
        </div>
      </div>
      </article>
      {scene && <script src="/demo/embed.js" defer />}
    </SiteFrame>
  );
}
