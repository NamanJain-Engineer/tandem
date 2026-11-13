import { siteButton } from "@tandem/ui/components/SiteButton";
import { ArrowRight, Download, Mail, MousePointerClick } from "lucide-react";
import { RELEASE } from "../../../lib/release";
import { EARLY_ACCESS_HREF, EMAIL_ME_HREF, SITE } from "../../../lib/site";
import { Icon } from "../Icon";
import "./cta.css";

/** The two rings at the size of a heading, drawn as hairlines. As the band
 * scrolls in they draw themselves (a stroke dash on a scroll timeline, under
 * @supports and only without reduced motion); otherwise they are simply
 * there. The light where they overlap is the band's one bloom. */
function Rings() {
  return (
    <svg aria-hidden="true" focusable="false" viewBox="0 0 176 112" fill="none" className="cta-rings">
      <defs>
        <linearGradient id="cta-ring-lit" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" style={{ stopColor: "var(--tn-accent)" }} />
          <stop offset="1" style={{ stopColor: "var(--tn-accent)", stopOpacity: 0.35 }} />
        </linearGradient>
        <radialGradient id="cta-lens" cx="0.5" cy="0.45" r="0.5">
          <stop offset="0" style={{ stopColor: "var(--site-glow-a)" }} />
          <stop offset="0.6" style={{ stopColor: "var(--site-glow-b)" }} />
          <stop offset="1" style={{ stopColor: "var(--site-glow-b)", stopOpacity: 0 }} />
        </radialGradient>
      </defs>
      {/* the light where the rings overlap */}
      <ellipse cx="88" cy="56" rx="24" ry="42" fill="url(#cta-lens)" className="cta-lens" />
      {/* Both start at the top: the left ring draws anticlockwise, the right one clockwise. */}
      <path
        d="M62 10a46 46 0 1 0 0 92a46 46 0 1 0 0-92"
        pathLength={100}
        className="cta-ring"
        stroke="url(#cta-ring-lit)"
      />
      <path
        d="M114 10a46 46 0 1 1 0 92a46 46 0 1 1 0-92"
        pathLength={100}
        className="cta-ring cta-ring-2"
        stroke="url(#cta-ring-lit)"
      />
    </svg>
  );
}

/** The closing call to action (DESIGN §5.1): the rings drawing in over one
 * soft bloom, the ask, one primary action and a short, honest line. On a
 * phone, where a desktop download is a dead end, the primary is the demo
 * and the second action emails the link, as in the hero (§5.1.3). */
export function CtaBand() {
  return (
    <section id="cta" aria-labelledby="cta-title" className="cta relative isolate overflow-hidden bg-page">
      <div aria-hidden="true" className="horizon absolute inset-x-0 top-0" />
      <div aria-hidden="true" className="cta-bloom" />
      <div className="container-site flex flex-col items-center gap-5 pt-16 pb-(--site-section-y) text-center">
        <Rings />
        <h2 id="cta-title" className="reveal text-sheen max-w-[16em] text-display-2xl text-fg">
          Try it on your own projects
        </h2>
        <p className="reveal stagger-1 max-w-measure text-lead text-pretty text-fg-muted">
          Free for Linux, no account. Tell us what works and what doesn&apos;t:{" "}
          <a href={EARLY_ACCESS_HREF} className="text-fg">
            {SITE.contactEmail}
          </a>
          .
        </p>
        <div className="reveal stagger-2 flex flex-wrap justify-center gap-3 pt-2 max-sm:w-full max-sm:max-w-80 max-sm:flex-col">
          <a href="/download/" className={siteButton("primary", "lg", "cta-primary max-sm:!hidden")}>
            <Icon icon={Download} size={18} />
            Download {RELEASE.version}
          </a>
          <a href="#try" className={siteButton("secondary", "lg", "max-sm:!hidden")}>
            Try it in your browser
          </a>
          <a href="#try" className={siteButton("primary", "lg", "cta-primary sm:!hidden")}>
            <Icon icon={MousePointerClick} size={18} />
            Try it in your browser
          </a>
          <a href={EMAIL_ME_HREF} className={siteButton("secondary", "lg", "sm:!hidden")}>
            <Icon icon={Mail} size={18} />
            Email me the link
          </a>
        </div>
        <p className="reveal stagger-3 pt-2 text-caption text-fg-muted">
          Built by {SITE.founder.name}.{" "}
          <a href="/about/" className="link-arrow inline-flex items-center gap-1 text-fg">
            More about {SITE.product}
            <Icon icon={ArrowRight} size={14} />
          </a>
        </p>
      </div>
    </section>
  );
}
