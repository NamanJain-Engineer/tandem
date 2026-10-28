import { siteButton } from "@tandem/ui/components/SiteButton";
import { Badge } from "@tandem/ui/components/Badge";
import { CATEGORIES } from "@tandem/protocol/changelog";
import { ArrowLeft, ArrowRight, Download } from "lucide-react";
import type { Metadata } from "next";
import type { ReactNode } from "react";
// The release pages' scenes and their styles (D-1880, D-1885, D-1888).
import "../../../components/site/scenes/scenes.css";
import { Scene } from "../../../components/site/scenes/Scene";
import { releaseScenes, type ReleaseScene } from "../../../components/site/scenes/releaseScenes";
import { Icon } from "../../../components/site/Icon";
import { InlineMarkdown } from "../../../components/site/InlineMarkdown";
import { SiteFrame } from "../../../components/site/SiteFrame";
import type { ChangelogEntry } from "@tandem/protocol/changelog";
import { PLATFORM_LABEL, RELEASES, addedCard, versionPath } from "../../../lib/changelog";
import { RELEASE } from "../../../lib/release";
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
  // Each of the release's scenes plays full width, directly above its own
  // Added card (D-1898), so the notes split around them.
  const added = release.entries.filter((e) => e.category === "Added");
  const slots = releaseScenes(release.version)
    .map((s) => {
      const i = added.findIndex((e) => addedCard(e.text).title.replace(/\.$/, "") === s.card);
      if (i < 0) throw new Error(`${release.version}: no Added card "${s.card}" for the ${s.id} scene`);
      return { scene: s, at: i };
    })
    .sort((a, b) => a.at - b.at);
  const scene = slots.length > 0;
  // The Added list's breaks: before each scene's card, and its end.
  const breaks = [...new Set([0, ...slots.map((s) => s.at), added.length])].sort((a, b) => a - b);
  const blocks: ReactNode[] = [];
  breaks.slice(0, -1).forEach((from, k) => {
    for (const s of slots) if (s.at === from) blocks.push(<ReleaseSceneBlock key={s.scene.id} scene={s.scene} since={release.version} />);
    const last = k === breaks.length - 2;
    const to = breaks[k + 1] ?? added.length;
    blocks.push(
      <div key={`notes-${from}`} className="flex min-w-0 flex-col gap-6 lg:col-span-8 lg:col-start-5">
        {to > from && <Cards entries={added.slice(from, to)} heading={from === 0} />}
        {last &&
          CATEGORIES.filter((c) => c !== "Added").map((c) => {
            const entries = release.entries.filter((e) => e.category === c);
            if (!entries.length) return null;
            return <Bullets key={c} title={c} entries={entries} />;
          })}
      </div>,
    );
  });
  return (
    <SiteFrame current="/changelog/">
      <article className="relative isolate">
      <div aria-hidden="true" className="bg-lens absolute inset-x-0 top-0 -z-10 h-[32rem]" />
      <div className="container-site grid gap-10 py-16 lg:grid-cols-12 lg:gap-6 lg:py-24">
        <aside className={`flex flex-col items-start gap-5 lg:sticky lg:top-24 lg:col-span-3 lg:self-start ${scene ? "" : "lg:row-span-2"}`}>
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
        </div>
        {blocks}
      </div>
      </article>
      {scene && <script src="/demo/embed.js" defer />}
    </SiteFrame>
  );
}

/** A release scene, full width, labelled with the release that brought it
 * (D-1886). No scroll reveal: it sits just under the summary, often at the
 * first screen's fold, where `rise` would leave it half faded at load (and
 * its text below contrast while it was). */
function ReleaseSceneBlock({ scene, since }: { scene: ReleaseScene; since: string }) {
  return (
    <Scene id={scene.id} text={scene.text()} caption={scene.caption} since={since} className="lg:col-span-12">
      {scene.scene()}
    </Scene>
  );
}

/** Added entries as cards, under the section's heading where the section
 * starts (the rest of it continues below a scene). */
function Cards({ entries, heading }: { entries: readonly ChangelogEntry[]; heading: boolean }) {
  return (
    <section className={heading ? "flex flex-col gap-5 border-t border-line pt-10" : "flex flex-col gap-5"}>
      {heading && <h2 className="eyebrow">Added</h2>}
      <ul className="grid gap-4">
        {entries.map((e) => (
          <li
            key={e.text}
            className="reveal surface flex flex-col gap-3 p-6 text-copy-sm leading-7 lg:p-7 text-fg-muted [&_code]:font-mono [&_code]:text-[0.875em] [&_code]:text-fg"
          >
            <AddedCard text={e.text} />
            <Platforms entry={e} />
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Changed, Fixed and Security entries as a list. */
function Bullets({ title, entries }: { title: string; entries: readonly ChangelogEntry[] }) {
  return (
    <section className="flex flex-col gap-5 border-t border-line pt-10">
      <h2 className="eyebrow">{title}</h2>
      <ul className="flex flex-col gap-3">
        {entries.map((e) => (
          <li
            key={e.text}
            className="ml-5 list-disc pl-1 text-copy-sm leading-7 text-fg-muted marker:text-fg-subtle [&_code]:font-mono [&_code]:text-[0.875em] [&_code]:text-fg [&_strong]:text-fg"
          >
            <p>
              <InlineMarkdown text={e.text} />
            </p>
            <Platforms entry={e} />
          </li>
        ))}
      </ul>
    </section>
  );
}

function Platforms({ entry }: { entry: ChangelogEntry }) {
  return (
    <span className="font-mono text-eyebrow uppercase tracking-[0.08em] text-fg-subtle">
      {entry.platforms.map((p) => PLATFORM_LABEL[p]).join(", ")}
    </span>
  );
}

/** An Added card's text (D-1887): its title and one-sentence lead in fg, the
 * detail below in fg-muted. */
function AddedCard({ text }: { text: string }) {
  const { title, lead, rest } = addedCard(text);
  return (
    <>
      <p className="text-fg">
        {title && <strong className="mb-1 block text-title text-fg">{title}</strong>}
        <InlineMarkdown text={lead} />
      </p>
      {rest && (
        <p className="text-fg-muted">
          <InlineMarkdown text={rest} />
        </p>
      )}
    </>
  );
}
