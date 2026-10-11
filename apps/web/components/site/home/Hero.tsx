import { buttonClass } from "@tandem/ui/components/Button";
import { ArrowRight, Download, Play } from "lucide-react";
import { versionPath } from "../../../lib/changelog";
import { formatSize, releaseAssets } from "../../../lib/manifest";
import { RELEASE } from "../../../lib/release";
import { SITE } from "../../../lib/site";
import { Icon } from "../Icon";

/** The phrase of the tagline drawn in the teal → sky highlight (og.png's treatment). */
const HIGHLIGHT = "coding agents";

/** The hero (§6.2): the release pill, the tagline as the h1, the lead, two
 * calls to action and a meta line. Nothing here animates in but the lead,
 * actions and meta: the h1 is the page's largest paint. */
export function Hero() {
  const deb = releaseAssets().get("linux")?.[0];
  const [before, after] = SITE.tagline.split(HIGHLIGHT);
  return (
    <section aria-label="Introduction" className="relative isolate overflow-hidden bg-page">
      <div aria-hidden="true" className="bg-site-grid absolute inset-0 -z-10 [mask-image:linear-gradient(to_bottom,black_40%,transparent)]" />
      <PaneSplit />
      <div className="container-site flex flex-col items-start gap-7 pb-14 pt-16 lg:pb-16 lg:pt-24">
        <a
          href={versionPath(RELEASE.version)}
          className="group inline-flex h-8 items-center gap-2.5 rounded-full border border-accent/40 bg-accent-soft pl-3 pr-2.5 text-caption text-fg"
        >
          <span aria-hidden="true" className="size-1.5 rounded-full bg-accent" />
          <span className="font-mono">Released · {RELEASE.version} for Linux</span>
          <span aria-hidden="true" className="h-3.5 w-px bg-accent/40" />
          <span className="link-arrow inline-flex items-center gap-1 font-medium text-accent">
            What&apos;s new
            <Icon icon={ArrowRight} size={14} />
          </span>
        </a>
        <h1 className="max-w-[17em] text-display-2xl text-balance text-fg">
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
        <p className="enter max-w-measure text-lead text-pretty text-fg-muted">
          Vibezeo runs Claude Code, Codex, Gemini CLI and other coding-agent CLIs side by side in real terminals,
          on your own subscriptions. One Dashboard shows every agent: what it&apos;s doing, and when it needs you.
        </p>
        <div className="enter enter-2 flex flex-wrap gap-3 pt-1">
          <a href="/download/" className={buttonClass("primary", "xl")}>
            <Icon icon={Download} size={18} />
            Download for Linux
          </a>
          <a href="/demo/" className={buttonClass("outline", "xl")}>
            Try it in your browser
          </a>
        </div>
        <p className="enter enter-3 flex flex-wrap items-center gap-x-3 gap-y-1 text-caption text-fg-muted">
          <span>Free for Linux. No account needed: it runs on your computer.</span>
          {deb && (
            <>
              <span aria-hidden="true">·</span>
              <span className="font-mono">
                .deb · {formatSize(deb.bytes)}
              </span>
            </>
          )}
          <span aria-hidden="true">·</span>
          <a href="#demo" className="inline-flex min-h-6 items-center gap-1.5 font-medium text-fg hover:text-accent">
            <Icon icon={Play} size={14} />
            Watch it run (1 min)
          </a>
        </p>
      </div>
    </section>
  );
}

/** Vibezeo's own backdrop motif: the hairlines of a 2×2 pane split with the
 * app's gutters, each pane wearing a status shape (working, waiting, done,
 * idle). Decorative; faint; desktop only. */
function PaneSplit() {
  const panes = [
    { x: 0, y: 0, s: "working" },
    { x: 1, y: 0, s: "waiting" },
    { x: 0, y: 1, s: "done" },
    { x: 1, y: 1, s: "idle" },
  ] as const;
  const W = 260;
  const H = 180;
  const G = 10;
  return (
    <svg
      aria-hidden="true"
      viewBox={`0 0 ${W * 2 + G} ${H * 2 + G}`}
      className="pointer-events-none absolute right-[max(1rem,calc((100vw-75rem)/2))] top-20 -z-10 hidden w-[34rem] text-fg opacity-[0.07] [mask-image:radial-gradient(closest-side,black_55%,transparent)] lg:block"
      fill="none"
    >
      {panes.map((p) => {
        const x = p.x * (W + G);
        const y = p.y * (H + G);
        return (
          <g key={p.s} transform={`translate(${x} ${y})`}>
            <rect x="0.5" y="0.5" width={W - 1} height={H - 1} rx="8" stroke="currentColor" />
            <line x1="0" y1="26" x2={W} y2="26" stroke="currentColor" />
            <g transform="translate(14 9)" fill="currentColor" stroke="currentColor">
              {p.s === "working" && (
                <>
                  <circle cx="2" cy="4" r="2" stroke="none" />
                  <circle cx="8" cy="4" r="2" stroke="none" />
                  <circle cx="14" cy="4" r="2" stroke="none" />
                </>
              )}
              {p.s === "waiting" && <path d="M4 -1 9 4 4 9 -1 4Z" stroke="none" />}
              {p.s === "done" && <path d="M0 4.5 3 7.5 9 0.5" fill="none" strokeWidth="2" />}
              {p.s === "idle" && <circle cx="4" cy="4" r="4" fill="none" strokeWidth="1.6" />}
            </g>
            {[44, 60, 76, 92].map((ly, i) => (
              <line key={ly} x1="14" y1={ly} x2={14 + (W - 28) * [0.82, 0.55, 0.7, 0.4][i]!} y2={ly} stroke="currentColor" strokeWidth="4" strokeLinecap="round" />
            ))}
          </g>
        );
      })}
    </svg>
  );
}
