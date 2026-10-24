import { docsPath } from "../../lib/docs/pages";
import { SITE } from "../../lib/site";

/** The header's links (and the phone menu's), in order. */
export const NAV = [
  { href: "/#features", label: "Features" },
  { href: "/demo/", label: "Demo" },
  { href: "/docs/", label: "Docs" },
  { href: "/changelog/", label: "Changelog" },
  { href: "/blog/", label: "Blog" },
  { href: "/about/", label: "About" },
] as const;

export type FooterLink = { href: string; label: string };

/** The footer's four columns, on every page (docs included). */
export const FOOTER: ReadonlyArray<{ title: string; links: readonly FooterLink[] }> = [
  {
    title: "Product",
    links: [
      { href: "/download/", label: "Download" },
      { href: "/demo/", label: "Live demo" },
      { href: "/changelog/", label: "Changelog" },
      { href: "/blog/", label: "Blog" },
      { href: "/#claude", label: "Built with Claude" },
    ],
  },
  {
    title: "Docs",
    links: [
      { href: docsPath(""), label: "Getting started" },
      { href: docsPath("code-mode"), label: "Code mode" },
      { href: docsPath("thread-mode"), label: "Thread mode" },
      { href: docsPath("agent-mode"), label: "Agent mode" },
      { href: docsPath("keyboard-shortcuts"), label: "Keyboard shortcuts" },
      { href: docsPath("troubleshooting"), label: "Troubleshooting" },
    ],
  },
  {
    title: "Company",
    links: [
      { href: "/about/", label: "About" },
      ...(SITE.founder.link ? [{ href: SITE.founder.link, label: `${SITE.founder.name} on X` }] : []),
      { href: `mailto:${SITE.contactEmail}`, label: SITE.contactEmail },
    ],
  },
  {
    title: "Legal",
    links: [
      { href: "/privacy/", label: "Privacy" },
      { href: "/terms/", label: "Terms" },
      { href: "/security/", label: "Security" },
    ],
  },
];
