import type { PageTree } from "fumadocs-core/server";
import { BookOpen, ChevronLeft, ChevronRight, X } from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";
import { navGroups, type NavGroup } from "../../lib/docs/nav";
import { Icon } from "../site/Icon";
import { SiteFrame } from "../site/SiteFrame";

export type TocItem = { title: ReactNode; url: string; depth: number };

const MENU_ID = "docs-menu";

/** The docs' frame (D-1839, D-1819): the site's header and footer, a sticky
 * sidebar of grouped pages, the page, and "On this page". Server-rendered:
 * the only script the docs run is Next's router, for links that open in the
 * page (D-1817). Phones get the sidebar from a popover. */
export function DocsShell({
  tree,
  url,
  toc,
  children,
}: {
  tree: PageTree.Root;
  url: string;
  toc: readonly TocItem[];
  children: ReactNode;
}) {
  const groups = navGroups(tree);
  const flat = groups.flatMap((g) => g.pages);
  const at = flat.findIndex((p) => p.url === url);
  const prev = at > 0 ? flat[at - 1] : undefined;
  const next = at >= 0 ? flat[at + 1] : undefined;
  return (
    <SiteFrame current="/docs/">
      <div className="relative isolate">
        <div aria-hidden="true" className="bg-lens absolute inset-x-0 top-0 -z-10 h-[26rem]" />
        <div className="container-site grid gap-x-12 pb-(--site-section-y) pt-8 lg:grid-cols-[14.5rem_minmax(0,1fr)] lg:pt-12 xl:grid-cols-[14.5rem_minmax(0,1fr)_12.5rem]">
          <aside
            id="nd-sidebar"
            aria-label="Docs"
            className="sticky top-[calc(var(--site-header-h)+2rem)] hidden max-h-[calc(100dvh-var(--site-header-h)-3rem)] self-start overflow-y-auto pb-6 lg:block"
          >
            <Tree groups={groups} url={url} />
          </aside>

          <div className="flex min-w-0 flex-col gap-8">
            <div className="lg:hidden">
              <button
                type="button"
                popoverTarget={MENU_ID}
                className="inline-flex h-10 items-center gap-2 rounded-md bg-(--site-glass) px-3.5 text-copy-sm font-medium text-fg shadow-[inset_0_0_0_1px_var(--site-line-strong)]"
              >
                <Icon icon={BookOpen} size={16} />
                Docs menu
              </button>
              <div
                id={MENU_ID}
                popover="auto"
                className="site-sheet fixed inset-x-0 top-0 m-0 h-auto max-h-dvh w-full max-w-none overflow-y-auto border-0 border-b border-line bg-page p-0 text-fg shadow-e3 backdrop:bg-overlay"
              >
                <div className="container-site flex flex-col gap-2 pb-6">
                  <div className="flex h-(--site-header-h) items-center justify-between">
                    <span className="text-title text-fg">Docs</span>
                    <button
                      type="button"
                      popoverTarget={MENU_ID}
                      popoverTargetAction="hide"
                      aria-label="Close the docs menu"
                      className="-mr-2 inline-flex size-10 items-center justify-center rounded-md hover:bg-(--site-glass)"
                    >
                      <Icon icon={X} size={20} />
                    </button>
                  </div>
                  <Tree groups={groups} url={url} />
                </div>
              </div>
            </div>

            <article className="docs-prose prose min-w-0 max-w-none">{children}</article>

            {(prev || next) && (
              <nav aria-label="Previous and next" className="grid gap-4 sm:grid-cols-2">
                {prev ? (
                  <Link href={prev.url} className="surface surface-hover flex flex-col gap-1 p-5">
                    <span className="flex items-center gap-1 text-caption text-fg-muted">
                      <Icon icon={ChevronLeft} size={14} />
                      Previous
                    </span>
                    <span className="text-copy font-medium text-fg">{prev.title}</span>
                  </Link>
                ) : (
                  <span />
                )}
                {next && (
                  <Link href={next.url} className="surface surface-hover flex flex-col items-end gap-1 p-5 text-right">
                    <span className="flex items-center gap-1 text-caption text-fg-muted">
                      Next
                      <Icon icon={ChevronRight} size={14} />
                    </span>
                    <span className="text-copy font-medium text-fg">{next.title}</span>
                  </Link>
                )}
              </nav>
            )}
          </div>

          {toc.length > 0 && (
            <aside
              id="nd-toc"
              aria-label="On this page"
              className="sticky top-[calc(var(--site-header-h)+2rem)] hidden max-h-[calc(100dvh-var(--site-header-h)-3rem)] self-start overflow-y-auto xl:block"
            >
              <p className="mb-3 font-mono text-[0.6875rem] uppercase tracking-[0.12em] text-fg-subtle">On this page</p>
              <ul className="flex flex-col border-l border-line">
                {toc.map((t) => (
                  <li key={t.url}>
                    <a
                      href={t.url}
                      className={`-ml-px block border-l border-transparent py-1.5 text-caption text-fg-muted transition-colors duration-(--dur-base) hover:border-accent hover:text-fg ${t.depth > 2 ? "pl-6" : "pl-3"}`}
                    >
                      {t.title}
                    </a>
                  </li>
                ))}
              </ul>
            </aside>
          )}
        </div>
      </div>
    </SiteFrame>
  );
}

function Tree({ groups, url }: { groups: NavGroup[]; url: string }) {
  return (
    <nav aria-label="Docs pages" className="flex flex-col gap-6">
      {groups.map((g) => (
        <div key={g.title || "pages"} className="flex flex-col gap-1">
          {g.title && (
            <p className="px-3 pb-1 font-mono text-[0.6875rem] uppercase tracking-[0.12em] text-fg-subtle">{g.title}</p>
          )}
          <ul className="flex flex-col">
            {g.pages.map((p) => {
              const active = p.url === url;
              return (
                <li key={p.url}>
                  <Link
                    href={p.url}
                    data-active={active ? "true" : "false"}
                    aria-current={active ? "page" : undefined}
                    className="flex min-h-9 items-center rounded-md px-3 py-1.5 text-copy-sm text-fg-muted transition-colors duration-(--dur-base) hover:bg-(--site-glass) hover:text-fg data-[active=true]:bg-[linear-gradient(90deg,var(--tn-accent-soft),transparent)] data-[active=true]:font-medium data-[active=true]:text-fg data-[active=true]:shadow-[inset_2px_0_0_var(--tn-accent)]"
                  >
                    {p.title}
                  </Link>
                </li>
              );
            })}
          </ul>
        </div>
      ))}
    </nav>
  );
}
