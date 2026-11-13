import { versionPath } from "../../lib/changelog";
import { RELEASE } from "../../lib/release";
import { SITE } from "../../lib/site";
import { Mark } from "./Mark";
import { FOOTER } from "./nav";
import "./footer.css";

/** The footer on every page (D-1836): a lit horizon, the brand and the
 * current release, four columns, then the owner line. No year, no bloom
 * (the closing band above it holds the page's light), nothing loud. */
export function SiteFooter() {
  return (
    <footer className="site-footer relative isolate bg-page">
      <div aria-hidden="true" className="horizon absolute inset-x-0 top-0" />
      <div className="container-site site-footer-grid">
        <div className="site-footer-brand">
          <a
            href="/"
            className="flex items-center gap-2.5 text-[1.0625rem] font-semibold tracking-[-0.02em] text-fg"
          >
            <Mark />
            {SITE.product}
          </a>
          <p className="max-w-[17rem] text-copy-sm text-pretty text-fg-muted">{SITE.tagline}</p>
          <a href={versionPath(RELEASE.version)} className="site-footer-version">
            <span aria-hidden="true" className="site-footer-dot" />
            Released · {RELEASE.version} for Linux
          </a>
        </div>
        <nav aria-label="Footer" className="contents">
          {FOOTER.map((col) => (
            <div key={col.title} className="site-footer-col">
              <h2 className="font-mono text-eyebrow uppercase tracking-[0.12em] text-fg-subtle">
                {col.title}
              </h2>
              <ul className="flex flex-col">
                {col.links.map((l) => (
                  <li key={l.href}>
                    <a href={l.href} className="site-footer-link">
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
        <div className="site-footer-base">
          <span>
            © {SITE.legal?.name ?? SITE.company} · Built by {SITE.founder.name}.
          </span>
          <span className="md:flex-1">
            Product names belong to their owners. {SITE.product} isn&apos;t affiliated with or endorsed by any
            engine vendor.
          </span>
          <span className="flex gap-4">
            <a href="/llms.txt" className="site-footer-link">
              llms.txt
            </a>
            <a href="/api/v1/site-info.json" className="site-footer-link">
              Site info
            </a>
          </span>
        </div>
      </div>
    </footer>
  );
}
