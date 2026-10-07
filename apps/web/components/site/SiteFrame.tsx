import type { ReactNode } from "react";
import { EARLY_ACCESS_HREF, SITE } from "../../lib/site";

/** Tandem's mark: two overlapping rings. */
export function Mark({ size = 22 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 28 28" fill="none" aria-hidden="true">
      <circle cx="10.5" cy="14" r="7" stroke="var(--tn-accent)" strokeWidth="2.2" />
      <circle cx="17.5" cy="14" r="7" stroke="var(--tn-accent)" strokeWidth="2.2" />
    </svg>
  );
}

const NAV = [
  { href: "/#demo", label: "Demo" },
  { href: "/#claude", label: "Claude" },
  { href: "/pricing/", label: "Pricing" },
  { href: "/changelog/", label: "Changelog" },
  { href: "/about/", label: "About" },
];

/** Header, main and footer for the public pages. Plain links: the site is
 * static and works without JavaScript. */
export function SiteFrame({ children }: { children: ReactNode }) {
  const year = new Date().getFullYear();
  return (
    <div className="flex min-h-screen flex-col bg-bg text-fg">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:left-3 focus:top-3 focus:rounded-sm focus:bg-surface-3 focus:px-3 focus:py-1.5"
      >
        Skip to content
      </a>
      <header className="border-b border-divider">
        <nav aria-label="Site" className="mx-auto flex h-16 max-w-5xl items-center gap-6 px-6 text-sm">
          <a href="/" className="flex items-center gap-2 text-md font-semibold text-fg">
            <Mark />
            {SITE.product}
          </a>
          <ul className="ml-auto hidden items-center gap-6 sm:flex">
            {NAV.map((n) => (
              <li key={n.href}>
                <a href={n.href} className="text-fg-muted hover:text-fg">
                  {n.label}
                </a>
              </li>
            ))}
          </ul>
          <a
            href={EARLY_ACCESS_HREF}
            className="ml-auto rounded-sm bg-accent px-3 py-1.5 font-medium text-accent-fg hover:bg-accent-hover sm:ml-0"
          >
            Early access
          </a>
        </nav>
      </header>
      <main id="main" className="flex-1">
        {children}
      </main>
      <footer className="border-t border-divider">
        <div className="mx-auto flex max-w-5xl flex-col gap-3 px-6 py-8 text-sm text-fg-muted sm:flex-row sm:items-center">
          <span>
            © {year} {SITE.legal?.name ?? SITE.company} · Built by {SITE.founder.name}. {SITE.product} is in
            active development.
          </span>
          <nav aria-label="Legal" className="flex flex-wrap gap-x-5 gap-y-2 sm:ml-auto">
            <a href="/about/" className="hover:text-fg">
              About
            </a>
            <a href="/changelog/" className="hover:text-fg">
              Changelog
            </a>
            <a href="/pricing/" className="hover:text-fg">
              Pricing
            </a>
            <a href="/privacy/" className="hover:text-fg">
              Privacy
            </a>
            <a href="/terms/" className="hover:text-fg">
              Terms
            </a>
            <a href="/security/" className="hover:text-fg">
              Security
            </a>
            <a href={`mailto:${SITE.contactEmail}`} className="hover:text-fg">
              {SITE.contactEmail}
            </a>
          </nav>
        </div>
      </footer>
    </div>
  );
}

/** A plain prose page (Privacy, Terms, Security). */
export function ProsePage({
  title,
  updated,
  children,
}: {
  title: string;
  updated?: string;
  children: ReactNode;
}) {
  return (
    <SiteFrame>
      <article className="mx-auto flex max-w-3xl flex-col gap-5 px-6 py-14 text-base leading-relaxed text-fg [&_h2]:mt-6 [&_h2]:text-lg [&_h2]:font-semibold [&_li]:ml-5 [&_li]:list-disc [&_p]:text-fg-muted [&_ul]:flex [&_ul]:flex-col [&_ul]:gap-1.5 [&_ul]:text-fg-muted">
        <header className="flex flex-col gap-2">
          <h1 className="text-3xl font-semibold tracking-tight">{title}</h1>
          {updated && <p className="text-sm">Last updated {updated}</p>}
        </header>
        {children}
      </article>
    </SiteFrame>
  );
}
