import { SITE } from "../../lib/site";

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
    <footer className="mt-12 flex flex-col gap-3 border-t border-border pt-6 text-caption text-fg-muted">
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
    </footer>
  );
}
