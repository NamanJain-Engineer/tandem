import { siteButton } from "@tandem/ui/components/SiteButton";
import { Download } from "lucide-react";
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
          Download {SITE.product} for Linux, then{" "}
          <a href={EARLY_ACCESS_HREF} className="text-accent">
            tell us what works and what doesn&apos;t
          </a>{" "}
          at {SITE.contactEmail}.
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
