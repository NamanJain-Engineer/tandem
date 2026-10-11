import { buttonClass } from "@tandem/ui/components/Button";
import { Download } from "lucide-react";
import { RELEASE } from "../../../lib/release";
import { EARLY_ACCESS_HREF, SITE } from "../../../lib/site";
import { Icon } from "../Icon";
import { Mark } from "../Mark";

/** The closing call to action: download, then tell us what works. */
export function CtaBand() {
  return (
    <section aria-labelledby="cta-title" className="relative isolate overflow-hidden border-t border-border bg-page">
      <div aria-hidden="true" className="bg-site-grid absolute inset-0 -z-10 [background-position:center] [mask-image:radial-gradient(60%_80%_at_50%_100%,black,transparent)]" />
      <div className="container-site flex flex-col items-center gap-6 py-(--site-section-y) text-center">
        <Mark size={44} glow />
        <h2 id="cta-title" className="text-display-xl text-balance text-fg">
          Try it on your own projects
        </h2>
        <p className="max-w-measure text-lead text-pretty text-fg-muted">
          Download {SITE.product} for Linux, then{" "}
          <a href={EARLY_ACCESS_HREF} className="text-accent">
            tell us what works and what doesn&apos;t
          </a>{" "}
          at {SITE.contactEmail}.
        </p>
        <div className="flex flex-wrap justify-center gap-3">
          <a href="/download/" className={buttonClass("primary", "xl")}>
            <Icon icon={Download} size={18} />
            Download {RELEASE.version}
          </a>
          <a href="/docs/" className={buttonClass("outline", "xl")}>
            Read the docs
          </a>
        </div>
      </div>
    </section>
  );
}
