import { DocsLayout } from "fumadocs-ui/layouts/docs";
import { RootProvider } from "fumadocs-ui/provider";
import type { ReactNode } from "react";
import { source } from "../../lib/docs/source";
import { SITE } from "../../lib/site";
import "./docs.css";

/** /docs (F-WB-02): Fumadocs, with no search service (the site is static
 * and calls nothing, D-132). */
export default function Layout({ children }: { children: ReactNode }) {
  return (
    <RootProvider search={{ enabled: false }} theme={{ enabled: false }}>
      <DocsLayout
        tree={source.pageTree}
        nav={{ title: `${SITE.product} docs`, url: "/docs/" }}
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
