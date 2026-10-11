import { SITE } from "../../lib/site";
import { Mark } from "./Mark";
import { FOOTER } from "./nav";

/** The footer on every page: four columns, then the owner line. No year. */
export function SiteFooter() {
  return (
    <footer className="border-t border-border bg-raised">
      <div className="container-site grid gap-10 py-14 md:grid-cols-[1.4fr_repeat(4,1fr)]">
        <div className="flex flex-col gap-3">
          <a href="/" className="flex items-center gap-2 self-start text-lg font-semibold tracking-tight text-fg">
            <Mark />
            {SITE.product}
          </a>
          <p className="max-w-[16rem] text-caption text-fg-muted">{SITE.tagline}</p>
        </div>
        <nav aria-label="Footer" className="contents">
          {FOOTER.map((col) => (
            <div key={col.title} className="flex flex-col gap-2">
              <h2 className="font-mono text-eyebrow uppercase tracking-[0.08em] text-fg-subtle">{col.title}</h2>
              <ul className="flex flex-col">
                {col.links.map((l) => (
                  <li key={l.href}>
                    <a
                      href={l.href}
                      className="inline-flex min-h-8 items-center text-copy-sm text-fg-muted hover:text-fg"
                    >
                      {l.label}
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </nav>
      </div>
      <div className="container-site flex flex-col gap-2 border-t border-divider py-6 text-caption text-fg-muted md:flex-row md:items-center md:gap-6">
        <span>
          © {SITE.legal?.name ?? SITE.company} · Built by {SITE.founder.name}.
        </span>
        <span className="md:flex-1">
          Product names belong to their owners. {SITE.product} isn&apos;t affiliated with or endorsed by any engine
          vendor.
        </span>
        <span className="flex gap-4">
          <a href="/llms.txt" className="hover:text-fg">
            llms.txt
          </a>
          <a href="/api/v1/site-info.json" className="hover:text-fg">
            Site info
          </a>
        </span>
      </div>
    </footer>
  );
}
