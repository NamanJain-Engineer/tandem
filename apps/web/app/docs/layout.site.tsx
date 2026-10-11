import { DocsLayout } from "fumadocs-ui/layouts/docs";
import { RootProvider } from "fumadocs-ui/provider";
import type { ReactNode } from "react";
import { source } from "../../lib/docs/source";
import { Mark } from "../../components/site/Mark";
import { SITE } from "../../lib/site";
import "./docs.css";

/** /docs (F-WB-02): Fumadocs, with no search service (the site is static
 * and calls nothing, D-132). */
export default function Layout({ children }: { children: ReactNode }) {
  return (
    <RootProvider search={{ enabled: false }} theme={{ enabled: false }}>
      <DocsLayout
        tree={source.pageTree}
        nav={{
          title: (
            <span className="inline-flex items-center gap-2 font-semibold">
              <Mark />
              {SITE.product}
              <span className="font-normal text-fg-muted">Docs</span>
            </span>
          ),
          url: "/docs/",
        }}
        themeSwitch={{ enabled: false }}
        links={[
          { text: SITE.product, url: "/" },
          { text: "Download", url: "/download/" },
          { text: "Pricing", url: "/pricing/" },
          { text: "Changelog", url: "/changelog/" },
        ]}
      >
        {children}
      </DocsLayout>
    </RootProvider>
  );
}
