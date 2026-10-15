import { siteButton } from "@tandem/ui/components/SiteButton";
import { Download } from "lucide-react";
import { RELEASE } from "../../lib/release";
import { SITE } from "../../lib/site";
import { Icon } from "../site/Icon";
import { Mark } from "../site/Mark";

/** The legal links and contact under every docs page, as on the rest of the
 * site (F-WB-01: every page has them). No year. */
export function DocsFooter() {
  const links = [
    { href: "/about/", label: "About" },
    { href: "/privacy/", label: "Privacy" },
    { href: "/terms/", label: "Terms" },
    { href: "/security/", label: "Security" },
    { href: `mailto:${SITE.contactEmail}`, label: SITE.contactEmail },
  ];
  return (
    <footer className="mt-16 flex flex-col gap-3 text-caption text-fg-muted">
      <div className="surface relative isolate mb-8 flex flex-col gap-5 overflow-hidden p-6 not-prose sm:flex-row sm:items-center sm:justify-between">
        <div aria-hidden="true" className="bg-lens absolute inset-0 -z-10 opacity-80" />
        <div className="flex items-center gap-4">
          <Mark size={28} />
          <div className="flex flex-col gap-0.5">
            <p className="text-title text-fg">
              {SITE.product} {RELEASE.version} for Linux
            </p>
            <p className="text-copy-sm text-fg-muted">Free, no account. Or try it in your browser first.</p>
          </div>
        </div>
        <div className="flex flex-wrap gap-2.5">
          <a href="/download/" className={siteButton("primary", "md")}>
            <Icon icon={Download} size={16} />
            Download
          </a>
          <a href="/demo/" className={siteButton("secondary", "md")}>
            Live demo
          </a>
        </div>
      </div>
      <div className="flex flex-col gap-3 border-t border-line pt-6">
      <nav aria-label="Legal" className="flex flex-wrap gap-x-5 gap-y-1">
        {links.map((l) => (
          <a key={l.href} href={l.href} className="inline-flex min-h-8 items-center hover:text-fg">
            {l.label}
          </a>
        ))}
      </nav>
      <p>
        © {SITE.legal?.name ?? SITE.company} · Product names belong to their owners. {SITE.product} isn&apos;t
        affiliated with or endorsed by any engine vendor.
      </p>
      </div>
    </footer>
  );
}
