import { siteButton } from "@tandem/ui/components/SiteButton";
import { ArrowRight, Download, Mail, Play } from "lucide-react";
import { versionPath } from "../../../lib/changelog";
import { formatSize, releaseAssets } from "../../../lib/manifest";
import { RELEASE } from "../../../lib/release";
import { HERO, OWNERSHIP } from "../../../lib/home";
import { EMAIL_ME_HREF, ENGINES } from "../../../lib/site";
import { iconFor } from "../icons";
import { Icon } from "../Icon";
import { EngineOrbit } from "../EngineOrbit";
import { Orbit } from "../Orbit";
import { Scene } from "../scenes/Scene";
import { WORKSPACE_CAPTION, WorkspaceScene, workspaceSceneText } from "../scenes/WorkspaceScene";

/** The hero (§6.2, D-1841, D-1881): ownership first, centred under the rings'
 * light. The release pill, the h1 (HERO), a one-sentence lead that counts
 * the engines from code, two calls to action and a meta line; then the
 * product itself, a scene of Code mode rising into the first viewport
 * (DESIGN §5.1), and the four ownership facts under it. The h1 paints first
 * and in solid ink: it's the page's largest paint (D-1840). */
export function Hero() {
  const deb = releaseAssets().get("linux")?.[0];
  return (
    <section aria-label="Introduction" className="relative isolate overflow-hidden bg-page">
      <div aria-hidden="true" className="bg-lens absolute inset-0 -z-10" />
      <Orbit
        id="hero"
        draw
        className="absolute left-1/2 top-0 -z-10 h-[54rem] w-[96rem] max-w-none -translate-x-1/2 -translate-y-[14rem] [mask-image:linear-gradient(to_bottom,black_35%,transparent_88%)]"
      />
      {/* The rings' centre: 380 of 760 viewBox units at 1.28 scale, less the 14 rem lift. */}
      <EngineOrbit top={262} radius={560} />
      <div className="container-site flex flex-col items-center gap-5 pb-16 pt-10 text-center lg:pb-20 lg:pt-10">
        <a
          href={versionPath(RELEASE.version)}
          className="group inline-flex h-8 items-center gap-2.5 rounded-full bg-(--site-glass) pl-3 pr-2.5 text-caption text-fg shadow-[inset_0_1px_0_var(--site-edge-hi),inset_0_0_0_1px_var(--site-line-strong)] transition-colors duration-(--dur-base) hover:bg-(--site-glass-hover)"
        >
          <span aria-hidden="true" className="size-1.5 rounded-full bg-fg-subtle" />
          <span className="font-mono text-[0.75rem]">Released · {RELEASE.version} for Linux</span>
          <span aria-hidden="true" className="h-3.5 w-px bg-line-strong" />
          <span className="link-arrow inline-flex items-center gap-1 font-medium text-fg">
            What&apos;s new
            <Icon icon={ArrowRight} size={14} />
          </span>
        </a>
        <h1 className="text-display-3xl text-fg">
          {HERO.title.map((line, i) => (
            <span key={line} className="block">
              {line === HERO.highlight ? <span className="text-ink">{line}</span> : line}
              {i < HERO.title.length - 1 ? " " : ""}
            </span>
          ))}
        </h1>
        <div className="flex flex-col items-center gap-2">
          <p className="max-w-[38rem] text-lead text-pretty text-fg-muted">
            Run the coding agents you already use side by side, in real terminals, and see at a glance which one
            needs you.
          </p>
          <a href="#engines" className="text-caption text-fg-muted underline decoration-[color-mix(in_oklab,currentColor_35%,transparent)] underline-offset-[0.2em] hover:text-fg">
            Works with {ENGINES.length} agent CLIs
          </a>
        </div>
        {/* On a phone a desktop download is a dead end: the demo leads, and
            the download link can be mailed to a computer (DESIGN §5.1, §9.1). */}
        <div className="enter enter-2 flex flex-wrap justify-center gap-3 pt-1">
          <a href="/download/" className={siteButton("primary", "lg", "max-sm:!hidden")}>
            <Icon icon={Download} size={18} />
            Download for Linux
          </a>
          <a href="#try" className={siteButton("secondary", "lg", "max-sm:!hidden")}>
            Try it in your browser
          </a>
          <a href="#try" className={siteButton("primary", "lg", "sm:!hidden")}>
            Try it in your browser
          </a>
          <a href={EMAIL_ME_HREF} className={siteButton("secondary", "lg", "sm:!hidden")}>
            <Icon icon={Mail} size={18} />
            Email me the link
          </a>
        </div>
        <p className="enter enter-4 flex flex-wrap items-center justify-center gap-x-3 gap-y-1 text-caption text-fg-muted">
          <span>Free</span>
          {deb && (
            <>
              <span aria-hidden="true" className="text-fg-subtle">·</span>
              <span className="font-mono text-[0.75rem]">.deb and .rpm · {formatSize(deb.bytes)}</span>
            </>
          )}
          <span aria-hidden="true" className="text-fg-subtle">·</span>
          <a href="/demo/#video" className="inline-flex min-h-6 items-center gap-1.5 font-medium text-fg hover:text-fg-muted">
            <Icon icon={Play} size={14} />
            Watch it run (1 min)
          </a>
        </p>
        <Scene id="workspace" text={workspaceSceneText()} caption={WORKSPACE_CAPTION} className="enter enter-4 mt-2 w-full max-w-stage">
          <WorkspaceScene />
        </Scene>
        <ul aria-label="What you keep" className="grid w-full max-w-[66rem] grid-cols-1 gap-2 pt-3 sm:grid-cols-2 lg:grid-cols-4 lg:whitespace-nowrap">
          {OWNERSHIP.map((o) => (
            <li
              key={o.text}
              className="flex items-center justify-center gap-2 rounded-full bg-(--site-glass) px-3.5 py-2 text-caption text-fg shadow-[inset_0_0_0_1px_var(--site-line)]"
            >
              <Icon icon={iconFor(o.icon)} size={14} className="shrink-0 text-fg-muted" />
              {o.text}
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
