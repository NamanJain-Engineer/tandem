import { blogPages } from "./blog/posts";
import { RELEASES, versionPath } from "./changelog";
import { DOCS_PAGES, docsPath } from "./docs/pages";
type Founder = { name: string; role: string; bio?: string; link?: string };
/** A registered company: its name, registration number (e.g. CIN) and
 * registered address. */
type Legal = { name: string; registration?: string; address?: string };

/** The public site at vibezeo.com (F-WB-01 slice). Contact and company
 * details come from the owner; nothing personal is published unless it's
 * listed here. */
export const SITE = {
  url: "https://vibezeo.com",
  product: "Vibezeo",
  tagline: "A desktop workspace for the coding agents you already use.",
  /** The name the site and the product are published under. */
  company: "Vibezeo",
  contactEmail: "contact@vibezeo.com",
  /** No analytics, ads or cookies (D-132). Anything non-essential added
   * here needs a consent banner that loads it only after the visitor agrees
   * (F-WB-01 AC2); the site tests fail until then. */
  analytics: null,
  /** Who runs the company and builds the product (home, About, Terms, Privacy). */
  founder: {
    name: "Naman Jain",
    role: "Founder",
    bio: "An engineer focused on building products and shipping meaningful software.",
    link: "https://x.com/jus_eng_njainam",
  } as Founder,
  /** When the company started (About); not shown when null. */
  founded: null as string | null,
  /** The registered entity, once there is one. When set, it's shown on About,
   * Terms and Privacy and in the footer; until then the founder runs the site. */
  legal: null as Legal | null,
  /** Where the company is based (About), when set. */
  location: "India" as string | null,
  /** Who serves the site (Privacy). */
  host: "Vercel",
} as const;

/** Who's responsible for the site and the app: the registered entity, or
 * "Vibezeo (Naman Jain)" until there is one. */
export const OPERATOR = SITE.legal?.name ?? `${SITE.company} (${SITE.founder.name})`;

/** The engine CLIs Vibezeo detects and runs (crates/tandem-engines/registry.json). */
export const ENGINES = [
  "Claude Code",
  "Codex",
  "Gemini CLI",
  "Copilot CLI",
  "Cursor Agent",
  "Grok",
  "OpenCode",
  "Droid",
  "Amp",
  "Aider",
  "Kimi",
  "Antigravity CLI",
  "Kiro CLI",
] as const;

/** The public pages, for the sitemap. */
export const SITE_PAGES = [
  "/",
  "/download/",
  "/demo/",
  "/about/",
  "/changelog/",
  "/privacy/",
  "/terms/",
  "/security/",
] as const;

/** Feedback from users: a prefilled email, so people can tell
 * us what they run. The site keeps no form or list of its own. */
export const EARLY_ACCESS_HREF = `mailto:${SITE.contactEmail}?subject=${encodeURIComponent(
  `${SITE.product} feedback`,
)}&body=${encodeURIComponent(
  "Hi,\n\nI'm trying Vibezeo.\n\nMy OS (macOS / Windows / Linux):\nCoding agents I use (e.g. Claude Code, Codex):\nWhat works, what doesn't:\n",
)}`;

/** `/.well-known/security.txt` (RFC 9116). `Expires` is 11 months after `now`
 * (the RFC recommends less than a year), so each deploy renews it. */
export function securityTxt(now: Date = new Date()): string {
  const expires = new Date(now);
  expires.setUTCMonth(expires.getUTCMonth() + 11);
  return [
    `Contact: mailto:${SITE.contactEmail}`,
    `Expires: ${expires.toISOString()}`,
    "Preferred-Languages: en",
    `Canonical: ${SITE.url}/.well-known/security.txt`,
    `Policy: ${SITE.url}/security/`,
    "",
  ].join("\n");
}

export function robotsTxt(): string {
  return `User-agent: *\nAllow: /\n\nSitemap: ${SITE.url}/sitemap.xml\n`;
}

export function sitemapXml(): string {
  const pages = [
    ...SITE_PAGES,
    ...DOCS_PAGES.map((p) => docsPath(p.slug)),
    ...RELEASES.map((r) => versionPath(r.version)),
    ...blogPages(),
  ];
  const urls = pages.map((p) => `  <url><loc>${SITE.url}${p}</loc></url>`).join("\n");
  return `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls}\n</urlset>\n`;
}
