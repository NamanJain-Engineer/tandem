import { siteButton } from "@tandem/ui/components/SiteButton";
import { ArrowRight, Download, Menu, X } from "lucide-react";
import { Icon } from "./Icon";
import { SITE } from "../../lib/site";
import { Mark } from "./Mark";
import { NAV } from "./nav";
import { ThemeSwitch } from "./ThemeSwitch";

const MENU_ID = "site-menu";

/** The sticky header (D-1836): glass over the page, a hairline that appears
 * once the page scrolls, the links centred. Below 1024 px they move into a
 * sheet built on the Popover API: no script, light dismiss and Esc for free.
 * Browsers without it still reach every link from the footer. The theme
 * control (D-1870) sits before Download, and in the sheet below 640 px. */
export function SiteHeader({ current }: { current?: string }) {
  const link = (href: string) =>
    current === href ? ({ "aria-current": "page" } as const) : ({} as Record<string, never>);
  return (
    <header className="site-header sticky top-0 z-50 border-b border-line bg-(--site-header) backdrop-blur-xl backdrop-saturate-150">
      <nav aria-label="Site" className="container-site relative flex h-(--site-header-h) items-center gap-2">
        <a
          href="/"
          className="-ml-2 flex h-10 items-center gap-2.5 rounded-md px-2 text-[1.0625rem] font-semibold tracking-[-0.02em] text-fg"
        >
          <Mark />
          {SITE.product}
        </a>
        <ul className="absolute left-1/2 hidden -translate-x-1/2 items-center gap-0.5 lg:flex">
          {NAV.map((n) => (
            <li key={n.href}>
              <a
                href={n.href}
                {...link(n.href)}
                className="hover-bar flex h-9 items-center rounded-md px-3.5 text-[0.875rem] font-medium text-fg-muted transition-colors duration-(--dur-base) hover:bg-(--site-glass) hover:text-fg aria-[current=page]:text-fg"
              >
                {n.label}
              </a>
            </li>
          ))}
        </ul>
        <ThemeSwitch name="theme" className="ml-auto max-sm:!hidden" />
        <a href="/download/" className={siteButton("secondary", "sm", "max-sm:ml-auto")}>
          <Icon icon={Download} size={15} />
          Download
        </a>
        <button
          type="button"
          popoverTarget={MENU_ID}
          aria-label="Menu"
          className="-mr-2 inline-flex size-10 items-center justify-center rounded-md text-fg hover:bg-(--site-glass) lg:hidden"
        >
          <Icon icon={Menu} size={20} />
        </button>
        <div
          id={MENU_ID}
          popover="auto"
          className="site-sheet fixed inset-x-0 top-0 m-0 h-auto max-h-dvh w-full max-w-none overflow-y-auto border-0 border-b border-line bg-page p-0 text-fg shadow-e3 backdrop:bg-overlay backdrop:backdrop-blur-sm"
        >
          <div className="container-site flex flex-col gap-3 pb-6">
            <div className="flex h-(--site-header-h) items-center justify-between">
              <span className="-ml-2 flex items-center gap-2.5 px-2 text-[1.0625rem] font-semibold tracking-[-0.02em]">
                <Mark />
                {SITE.product}
              </span>
              <button
                type="button"
                popoverTarget={MENU_ID}
                popoverTargetAction="hide"
                aria-label="Close menu"
                className="-mr-2 inline-flex size-10 items-center justify-center rounded-md hover:bg-(--site-glass)"
              >
                <Icon icon={X} size={20} />
              </button>
            </div>
            <ul aria-label="Site menu" className="flex flex-col">
              {[{ href: "/", label: "Home" }, ...NAV].map((n) => (
                <li key={n.href} className="border-b border-line last:border-0">
                  <a
                    href={n.href}
                    {...link(n.href)}
                    className="group flex h-14 items-center justify-between text-[1.125rem] font-medium tracking-[-0.01em] text-fg-muted hover:text-fg aria-[current=page]:text-fg"
                  >
                    {n.label}
                    <Icon icon={ArrowRight} size={18} className="text-fg-subtle transition-transform duration-(--dur-base) group-hover:translate-x-0.5" />
                  </a>
                </li>
              ))}
            </ul>
            <div className="theme-only flex items-center justify-between gap-4 sm:hidden">
              <span className="text-copy-sm text-fg-muted">Theme</span>
              <ThemeSwitch name="theme-menu" labels />
            </div>
            <a href="/download/" {...link("/download/")} className={siteButton("primary", "lg", "w-full")}>
              <Icon icon={Download} size={18} />
              Download for Linux
            </a>
          </div>
        </div>
      </nav>
    </header>
  );
}
