import type { ReactNode } from "react";
import { SiteFooter } from "./SiteFooter";
import { SiteHeader } from "./SiteHeader";

export { Mark } from "./Mark";

/** Header, main and footer for the public pages. Plain links: the site is
 * static and works without JavaScript. `current` marks the header link of the
 * page being shown. */
export function SiteFrame({ children, current }: { children: ReactNode; current?: string }) {
  return (
    <div className="flex min-h-screen flex-col bg-page text-fg">
      <a
        href="#main"
        className="sr-only z-[60] focus:not-sr-only focus:fixed focus:left-3 focus:top-3 focus:rounded-sm focus:bg-card focus:px-3 focus:py-2"
      >
        Skip to content
      </a>
      <SiteHeader current={current} />
      <main id="main" className="flex-1">
        {children}
      </main>
      <SiteFooter />
    </div>
  );
}

/** A plain prose page (Privacy, Terms, Security). */
export function ProsePage({
  title,
  eyebrow,
  updated,
  children,
}: {
  title: string;
  eyebrow?: string;
  updated?: string;
  children: ReactNode;
}) {
  return (
    <SiteFrame>
      <article className="container-site py-(--site-section-y)">
        <div className="mx-auto flex max-w-prose flex-col gap-5 text-copy text-fg-muted [&_a]:text-accent [&_a]:underline-offset-4 hover:[&_a]:underline [&_h2]:mt-8 [&_h2]:text-display-md [&_h2]:text-fg [&_h3]:mt-4 [&_h3]:text-title [&_h3]:text-fg [&_li]:ml-5 [&_li]:list-disc [&_li]:pl-1 [&_strong]:text-fg [&_ul]:flex [&_ul]:flex-col [&_ul]:gap-2">
          <header className="flex flex-col gap-4 border-b border-border pb-8">
            {eyebrow && <p className="eyebrow">{eyebrow}</p>}
            <h1 className="text-display-xl text-balance text-fg">{title}</h1>
            {updated && <p className="text-caption text-fg-muted">Last updated {updated}</p>}
          </header>
          {children}
        </div>
      </article>
    </SiteFrame>
  );
}
