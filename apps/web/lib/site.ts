type Founder = { name: string; role: string; link?: string };

/** The public site at vibezeo.com (F-WB-01 slice). Contact and company
 * details come from the owner; nothing personal is published unless it's
 * listed here. */
export const SITE = {
  url: "https://vibezeo.com",
  product: "Tandem",
  tagline: "A desktop workspace for the coding agents you already use.",
  /** The name the site is published under (footer, Terms, Privacy). There's
   * no registered entity yet: the founder runs it. */
  company: "Vibezeo",
  contactEmail: "hello@vibezeo.com",
  /** Who runs the site and builds the product (home page, Terms, Privacy). */
  founder: { name: "Naman Jain", role: "Founder" } as Founder,
  updated: "7 October 2026",
} as const;

/** "Vibezeo (Naman Jain)": who's responsible for the site and the app. */
export const OPERATOR = `${SITE.company} (${SITE.founder.name})`;

/** The engine CLIs Tandem detects and runs (crates/tandem-engines/registry.json). */
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
] as const;

/** The public pages, for the sitemap. */
export const SITE_PAGES = ["/", "/privacy/", "/terms/", "/security/"] as const;

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
  const urls = SITE_PAGES.map((p) => `  <url><loc>${SITE.url}${p}</loc></url>`).join("\n");
  return `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls}\n</urlset>\n`;
}
