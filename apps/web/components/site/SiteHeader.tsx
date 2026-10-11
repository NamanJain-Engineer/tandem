import { buttonClass } from "@tandem/ui/components/Button";
import { Download, Menu, X } from "lucide-react";
import { Icon } from "./Icon";
import { SITE } from "../../lib/site";
import { Mark } from "./Mark";
import { NAV } from "./nav";

const MENU_ID = "site-menu";

/** The sticky header. Below 1024 px the links move into a menu built on the
 * Popover API: no script, light dismiss and Esc for free. Browsers without it
 * still reach every link from the footer. */
export function SiteHeader({ current }: { current?: string }) {
  const link = (href: string) =>
    current === href ? ({ "aria-current": "page" } as const) : ({} as Record<string, never>);
  return (
    <header className="site-header sticky top-0 z-50 border-b border-border bg-(--site-header) backdrop-blur-md backdrop-saturate-150">
      <nav aria-label="Site" className="container-site flex h-(--site-header-h) items-center gap-2">
        <a
          href="/"
          className="-ml-2 flex h-10 items-center gap-2 rounded-sm px-2 text-lg font-semibold tracking-tight text-fg"
        >
          <Mark />
          {SITE.product}
        </a>
        <ul className="ml-6 hidden items-center gap-1 lg:flex">
          {NAV.map((n) => (
            <li key={n.href}>
              <a
                href={n.href}
                {...link(n.href)}
                className="flex h-10 items-center rounded-sm px-3 text-copy-sm text-fg-muted transition-colors duration-(--dur-instant) hover:text-fg aria-[current=page]:text-fg"
              >
                {n.label}
              </a>
            </li>
          ))}
        </ul>
        <a href="/download/" className={`${buttonClass("primary", "lg")} ml-auto`}>
          <Icon icon={Download} size={16} />
          Download
        </a>
        <button
          type="button"
          popoverTarget={MENU_ID}
          aria-label="Menu"
          className="-mr-2 inline-flex size-10 items-center justify-center rounded-sm text-fg hover:bg-surface-3 lg:hidden"
        >
          <Icon icon={Menu} size={20} />
        </button>
        <div
          id={MENU_ID}
          popover="auto"
          className="m-0 ml-auto h-dvh max-h-none w-[min(20rem,85vw)] border-0 border-l border-border bg-card p-0 text-fg shadow-3 backdrop:bg-overlay"
        >
          <div className="flex h-full flex-col gap-2 p-4">
            <div className="flex items-center justify-between">
              <span className="flex items-center gap-2 px-2 text-lg font-semibold">
                <Mark />
                {SITE.product}
              </span>
              <button
                type="button"
                popoverTarget={MENU_ID}
                popoverTargetAction="hide"
                aria-label="Close menu"
                className="inline-flex size-10 items-center justify-center rounded-sm hover:bg-surface-3"
              >
                <Icon icon={X} size={20} />
              </button>
            </div>
            <ul aria-label="Site menu" className="flex flex-col">
              {[{ href: "/", label: "Home" }, ...NAV, { href: "/download/", label: "Download" }].map((n) => (
                <li key={n.href}>
                  <a
                    href={n.href}
                    {...link(n.href)}
                    className="flex h-12 items-center rounded-sm px-2 text-copy text-fg-muted hover:bg-surface-3 hover:text-fg aria-[current=page]:text-fg"
                  >
                    {n.label}
                  </a>
                </li>
              ))}
            </ul>
          </div>
        </div>
      </nav>
    </header>
  );
}
