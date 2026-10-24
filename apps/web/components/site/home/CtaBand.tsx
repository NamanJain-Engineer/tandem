import { siteButton } from "@tandem/ui/components/SiteButton";
import { ArrowRight, Download } from "lucide-react";
import { RELEASE } from "../../../lib/release";
import { EARLY_ACCESS_HREF, SITE } from "../../../lib/site";
import { CtaBand as Band } from "../CtaBand";
import { Icon } from "../Icon";

/** The closing call to action: download, then tell us what works. */
export function CtaBand() {
  return (
    <Band
      id="cta"
      title="Try it on your own projects"
      lead={
        <>
          Free for Linux, no account. Tell us what works and what doesn&apos;t:{" "}
          <a href={EARLY_ACCESS_HREF} className="text-fg">
            {SITE.contactEmail}
          </a>
          .
        </>
      }
      note={
        <>
          Built by {SITE.founder.name}.{" "}
          <a href="/about/" className="link-arrow inline-flex items-center gap-1 text-fg">
            More about {SITE.product}
            <Icon icon={ArrowRight} size={14} />
          </a>
        </>
      }
    >
      <a href="/download/" className={siteButton("primary", "lg")}>
        <Icon icon={Download} size={18} />
        Download {RELEASE.version}
      </a>
      <a href="/docs/" className={siteButton("secondary", "lg")}>
        Read the docs
      </a>
    </Band>
  );
}
