import { siteButton } from "@tandem/ui/components/SiteButton";
import { ArrowRight, Download, Play } from "lucide-react";
import { versionPath } from "../../../lib/changelog";
import { formatSize, releaseAssets } from "../../../lib/manifest";
import { RELEASE } from "../../../lib/release";
import { SITE } from "../../../lib/site";
import { Icon } from "../Icon";
import { Orbit } from "../Orbit";

/** The phrase of the tagline drawn in the teal → sky highlight (og.png's treatment). */
const HIGHLIGHT = "coding agents";

/** The hero (§6.2, D-1837): centred under the rings' light. The release
 * pill, the tagline as the h1, the lead, two calls to action and a meta line. Nothing here animates in but the lead,
 * actions and meta: the h1 is the page's largest paint. */
export function Hero() {
  const deb = releaseAssets().get("linux")?.[0];
  const [before, after] = SITE.tagline.split(HIGHLIGHT);
  return (
    <section aria-label="Introduction" className="grain relative isolate overflow-hidden bg-page">
      <div aria-hidden="true" className="bg-lens absolute inset-0 -z-10" />
      <Orbit
        id="hero"
        className="absolute left-1/2 top-0 -z-10 h-[54rem] w-[96rem] max-w-none -translate-x-1/2 -translate-y-[14rem] [mask-image:linear-gradient(to_bottom,black_35%,transparent_88%)]"
      />
      <div className="container-site flex flex-col items-center gap-7 pb-16 pt-20 text-center lg:pb-20 lg:pt-28">
        <a
          href={versionPath(RELEASE.version)}
          className="group inline-flex h-8 items-center gap-2.5 rounded-full bg-(--site-glass) pl-3 pr-2.5 text-caption text-fg shadow-[inset_0_1px_0_var(--site-edge-hi),inset_0_0_0_1px_var(--site-line-strong)] backdrop-blur-md transition-colors duration-(--dur-base) hover:bg-(--site-glass-hover)"
        >
          <span aria-hidden="true" className="size-1.5 rounded-full bg-accent shadow-[0_0_10px_var(--tn-accent)]" />
          <span className="font-mono text-[0.75rem]">Released · {RELEASE.version} for Linux</span>
          <span aria-hidden="true" className="h-3.5 w-px bg-line-strong" />
          <span className="link-arrow inline-flex items-center gap-1 font-medium text-accent">
            What&apos;s new
            <Icon icon={ArrowRight} size={14} />
          </span>
        </a>
        <h1 className="text-sheen max-w-[15em] text-display-3xl text-fg [text-wrap:pretty]">
          {after === undefined ? (
            SITE.tagline
          ) : (
            <>
              {before}
              <span className="text-ink">{HIGHLIGHT}</span>
              {after}
            </>
          )}
        </h1>
        <p className="enter max-w-[40rem] text-lead text-pretty text-fg-muted">
          Vibezeo runs Claude Code, Codex, Gemini CLI and other coding-agent CLIs side by side in real terminals,
          on your own subscriptions. One Dashboard shows every agent: what it&apos;s doing, and when it needs you.
        </p>
        <div className="enter enter-2 flex flex-wrap justify-center gap-3 pt-1">
          <a href="/download/" className={siteButton("primary", "lg")}>
            <Icon icon={Download} size={18} />
            Download for Linux
          </a>
          <a href="/demo/" className={siteButton("secondary", "lg")}>
            Try it in your browser
          </a>
        </div>
        <p className="enter enter-3 flex flex-wrap items-center justify-center gap-x-3 gap-y-1 text-caption text-fg-muted">
          <span>Free for Linux. No account needed: it runs on your computer.</span>
          {deb && (
            <>
              <span aria-hidden="true" className="text-fg-subtle">·</span>
              <span className="font-mono text-[0.75rem]">.deb · {formatSize(deb.bytes)}</span>
            </>
          )}
          <span aria-hidden="true" className="text-fg-subtle">·</span>
          <a href="#demo" className="inline-flex min-h-6 items-center gap-1.5 font-medium text-fg hover:text-accent">
            <Icon icon={Play} size={14} />
            Watch it run (1 min)
          </a>
        </p>
      </div>
    </section>
  );
}
