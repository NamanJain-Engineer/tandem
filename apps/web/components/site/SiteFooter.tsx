import { versionPath } from "../../lib/changelog";
import { RELEASE } from "../../lib/release";
import { SITE } from "../../lib/site";
import { Mark } from "./Mark";
import { FOOTER } from "./nav";

/** The footer on every page (D-1836): a lit horizon, the brand and the
 * current release, four columns, then the owner line. No year. */
export function SiteFooter() {
  return (
    <footer className="relative isolate overflow-hidden bg-page">
      <div aria-hidden="true" className="horizon absolute inset-x-0 top-0" />
      <div aria-hidden="true" className="bg-lens absolute inset-x-0 top-0 -z-10 h-64 opacity-60" />
      <div className="container-site grid gap-12 pb-12 pt-16 md:grid-cols-2 lg:grid-cols-[1.6fr_repeat(4,1fr)] lg:pt-20">
        <div className="flex flex-col items-start gap-4">
          <a href="/" className="flex items-center gap-2.5 text-[1.0625rem] font-semibold tracking-[-0.02em] text-fg">
            <Mark />
            {SITE.product}
          </a>
          <p className="max-w-[17rem] text-copy-sm text-fg-muted">{SITE.tagline}</p>
          <a
            href={versionPath(RELEASE.version)}
            className="inline-flex h-7 items-center gap-2 rounded-full bg-(--site-glass) px-3 font-mono text-[0.75rem] text-fg-muted shadow-[inset_0_0_0_1px_var(--site-line)] hover:text-fg"
          >
            <span aria-hidden="true" className="size-1.5 rounded-full bg-accent shadow-[0_0_8px_var(--tn-accent)]" />
            Released · {RELEASE.version} for Linux
          </a>
        </div>
        <nav aria-label="Footer" className="contents">
          {FOOTER.map((col) => (
            <div key={col.title} className="flex flex-col gap-3">
              <h2 className="font-mono text-eyebrow uppercase tracking-[0.12em] text-fg-subtle">{col.title}</h2>
              <ul className="flex flex-col gap-0.5">
                {col.links.map((l) => (
                  <li key={l.href}>
                    <a
                      href={l.href}
                      className="inline-flex min-h-8 items-center text-copy-sm text-fg-muted transition-colors duration-(--dur-base) hover:text-fg"
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
      <div className="container-site">
        <div className="flex flex-col gap-2 border-t border-line py-7 text-caption text-fg-muted md:flex-row md:items-center md:gap-6">
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
      </div>
    </footer>
  );
}
