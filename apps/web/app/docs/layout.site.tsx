import type { ReactNode } from "react";
import "./docs.css";

/** /docs (F-WB-02): each page draws its own frame (`DocsShell`), so the
 * sidebar marks it on the server. No search service (the site is static and
 * calls nothing, D-132) and no client layout (D-1819). */
export default function Layout({ children }: { children: ReactNode }) {
  return children;
}
