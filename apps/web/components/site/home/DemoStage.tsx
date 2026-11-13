import { siteButton } from "@tandem/ui/components/SiteButton";
import { Maximize2 } from "lucide-react";
import { DEMO } from "../../../lib/home";
import { STILL_ALT } from "../../../lib/demoStill";
import { SITE } from "../../../lib/site";
import { Icon } from "../Icon";
import { LitGlow } from "../LitGlow";
import "./demo-stage.css";

/** The stage's markup (D-128): a still image until /demo/embed.js frames the
 * live demo here. Inner HTML, so hydration never replaces what the loader
 * puts in it. The still is the current release's demo (OPERATING §6). It
 * loads lazily: on a phone it's below the fold, and fetched with the page it
 * held up the h1, the largest paint, on a slow connection (D-1816). */
const DEMO_STAGE = `<div data-demo-stage="" data-stage="" class="absolute inset-0 overflow-hidden rounded-[10px] bg-stage">
<div data-demo-bar="" hidden class="absolute inset-x-0 top-0 flex h-10 items-center gap-3 border-b border-stage-line bg-stage px-4 text-sm text-stage-muted">
<span>Interactive demo · simulated agents, sample data</span>
<button type="button" data-demo-close="" class="ml-auto rounded-sm px-3 py-1 font-medium text-stage-fg hover:bg-white/10">Close (Esc)</button>
</div>
<picture><source type="image/avif" srcset="/demo/still-800.avif 800w, /demo/still.avif 1280w, /demo/still@2x.avif 2560w" sizes="(min-width: 1312px) 1268px, 100vw"><source type="image/webp" srcset="/demo/still-800.webp 800w, /demo/still.webp 1280w, /demo/still@2x.webp 2560w" sizes="(min-width: 1312px) 1268px, 100vw"><img data-demo-still="" src="/demo/still.jpg" width="1280" height="800" loading="lazy" decoding="async" alt="${STILL_ALT}" class="block h-full w-full"></picture>
<a href="/demo/app/" data-demo-open="" class="absolute inset-0 flex items-center justify-center bg-black/40 text-md font-semibold lg:hidden"><span class="btn btn-primary btn-md">Tap to explore</span></a>
</div>`;

/** #try (§6.3): the live demo, right under the hero, in a lit frame that
 * catches the pointer, with an invitation beside its title: a status light that
 * comes on once the app is running in the frame (demo-stage.css), "click
 * anything", and Explore full screen. `id` stays the section's first
 * attribute and nothing nests a <section> inside it: the site test cuts it
 * out by those to check "sample data" appears only here. */
export function DemoStage() {
  return (
    <section
      id="try"
      aria-labelledby="try-title"
      className="demo-section relative scroll-mt-(--site-header-h) bg-page pb-(--site-section-y)"
    >
      <div aria-hidden="true" className="demo-bloom absolute inset-x-0 top-24 h-[44rem]" />
      {/* No stacking context or containing block round the stage (isolate,
          z-index, transform, filter): the demo's full screen is position:
          fixed at z-index 1000 and must rise above the sticky header. The
          lit frame's own (light.css) stands down while it's full screen. */}
      <div className="relative mx-auto flex w-full max-w-stage flex-col gap-5 px-(--site-gutter)">
        <div className="flex flex-wrap items-end justify-between gap-4">
          <div className="flex flex-col gap-3">
            <span className="inline-flex h-7 items-center gap-2 self-start rounded-full bg-(--site-glass) px-3 font-mono text-[0.6875rem] uppercase tracking-[0.12em] text-fg-muted shadow-[inset_0_0_0_1px_var(--site-line-strong)]">
              <span aria-hidden="true" className="size-1.5 rounded-full bg-fg-subtle" />
              {DEMO.label}
            </span>
            <h2 id="try-title" className="text-sheen text-display-md text-fg">
              Try {SITE.product} right here
            </h2>
          </div>
          <div className="demo-invite hidden items-center gap-5 lg:flex">
            <p className="flex items-center gap-2.5 text-copy-sm text-fg-muted">
              <span aria-hidden="true" className="demo-live" />
              <span className="font-medium text-fg">{DEMO.live}</span>
              <span aria-hidden="true" className="text-fg-subtle">
                ·
              </span>
              {DEMO.invite}
            </p>
            <a
              href="/demo/app/"
              data-demo-open=""
              className={siteButton("secondary", "md", "icon-turn demo-full")}
            >
              <Icon icon={Maximize2} size={16} />
              {DEMO.full}
            </a>
          </div>
        </div>
        <div data-lit="" className="demo-frame frame">
          <LitGlow />
          {/* D-128: /demo/embed.js frames /demo/app/ here once the section is near. */}
          <div
            data-demo-embed=""
            className="relative w-full"
            style={{ aspectRatio: "1280 / 800" }}
            dangerouslySetInnerHTML={{ __html: DEMO_STAGE }}
          />
        </div>
        <p className="max-w-[46rem] text-caption text-pretty text-fg-muted">{DEMO.note}</p>
      </div>
      <script src="/demo/embed.js" defer />
    </section>
  );
}
