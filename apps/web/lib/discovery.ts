// Machine-readable discovery (F-WB-04): /llms.txt, /llms-full.txt,
// /api/v1/site-info and the OpenAPI document that describes it. All of it is
// built from the same config as /pricing, /download and /docs.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { PRICES } from "@tandem/protocol/plans";
import { BLOG_PATH, FEED_PATH, POSTS } from "./blog/posts";
import { DOCS_MARKDOWN } from "./docs/tables";
import { DOCS_PAGES, docsPath } from "./docs/pages";
import { releaseAssets } from "./manifest";
import { CREDIT_ROWS, CREDIT_VALUE_USD, PLANS } from "./pricing";
import { CHECKSUMS_URL, DOWNLOADS, RELEASE } from "./release";
import { ENGINES, SITE } from "./site";

/** Served from `site-info.json` in the export: Vercel rewrites the path to it
 * after the site's trailing-slash redirect (vercel.json, D-135). */
export const SITE_INFO_PATH = "/api/v1/site-info";

const Download = z.object({
  file: z.string(),
  url: z.url(),
  sha256: z.string().regex(/^[0-9a-f]{64}$/),
});

export const SiteInfoSchema = z
  .object({
    name: z.string(),
    url: z.url(),
    version: z.string().describe("The released version of the desktop app."),
    platforms: z.array(
      z.object({
        os: z.enum(["macos", "windows", "linux"]),
        name: z.string(),
        requirement: z.string(),
        available: z.boolean(),
        downloads: z.array(Download),
      }),
    ),
    pricing: z.object({
      currency: z.literal("USD"),
      url: z.url(),
      plans: z.array(
        z.object({
          id: z.enum(["free", "pro", "team"]),
          name: z.string(),
          available: z.boolean().describe("False until the plan can be bought."),
          monthlyCents: z.number().int(),
          yearlyCents: z.number().int().nullable(),
          perSeat: z.boolean(),
          minSeats: z.number().int().nullable(),
          trialDays: z.number().int().nullable(),
        }),
      ),
      creditValueUsd: z
        .number()
        .describe("What one credit is worth; credits pay only for hosted AI features."),
      creditPrices: z
        .record(z.string(), z.number().int())
        .describe("Credits per use of each hosted feature."),
    }),
  })
  .describe("Public, read-only facts about Vibezeo.");

export type SiteInfo = z.infer<typeof SiteInfoSchema>;

const abs = (path: string) => `${SITE.url}${path}`;

export function siteInfo(): SiteInfo {
  const assets = releaseAssets();
  return {
    name: SITE.product,
    url: abs("/"),
    version: RELEASE.version,
    platforms: DOWNLOADS.map((d) => ({
      os: d.os,
      name: d.name,
      requirement: d.requirement,
      available: d.available,
      downloads: (assets.get(d.os) ?? []).map(({ file, url, sha256 }) => ({ file, url, sha256 })),
    })),
    pricing: {
      currency: "USD",
      url: abs("/pricing/"),
      plans: [
        {
          id: "free",
          name: "Free",
          available: true,
          monthlyCents: 0,
          yearlyCents: null,
          perSeat: false,
          minSeats: null,
          trialDays: null,
        },
        {
          id: "pro",
          name: "Pro",
          available: false,
          monthlyCents: PRICES.pro.month,
          yearlyCents: PRICES.pro.year,
          perSeat: false,
          minSeats: null,
          trialDays: PRICES.pro.trialDays,
        },
        {
          id: "team",
          name: "Team",
          available: false,
          monthlyCents: PRICES.team.month,
          yearlyCents: PRICES.team.year,
          perSeat: true,
          minSeats: PRICES.team.minSeats,
          trialDays: PRICES.team.trialDays,
        },
      ],
      creditValueUsd: CREDIT_VALUE_USD,
      creditPrices: Object.fromEntries(CREDIT_ROWS.map((r) => [r.key, r.credits])),
    },
  };
}

type DocSource = { slug: string; title: string; description: string; body: string };

/** A docs page's MDX source, split into its front matter and body. */
export function docSource(slug: string): DocSource {
  const raw = readFileSync(join(process.cwd(), "content/docs", `${slug || "index"}.mdx`), "utf8");
  const m = /^---\n([\s\S]*?)\n---\n([\s\S]*)$/.exec(raw);
  const front = m?.[1] ?? "";
  const field = (k: string) => new RegExp(`^${k}: (.*)$`, "m").exec(front)?.[1]?.trim() ?? "";
  return { slug, title: field("title"), description: field("description"), body: (m?.[2] ?? raw).trim() };
}

/** A page for a feature the release doesn't have opens with this label. */
export const notInApp = (d: DocSource) => d.body.startsWith("> **Not in the app yet.**");

/** A page for a feature that's in the app but runs only on Pro, which the
 * download can't have, opens with this one. */
export const partOfPro = (d: DocSource) => d.body.startsWith("> **Part of Pro.**");

/** MDX to plain Markdown: generated components become their tables, site
 * links become absolute, and `<kbd>` keys become code. */
function toMarkdown(body: string): string {
  return body
    .replace(/<([A-Z]\w*) \/>/g, (tag, name: string) => DOCS_MARKDOWN[name]?.() ?? tag)
    .replace(/<kbd>(.*?)<\/kbd>/g, "`$1`")
    .replace(/\]\((\/[^)]*)\)/g, (_, path: string) => `](${abs(path)})`);
}

const planLine = (id: string) => {
  const p = PLANS.find((x) => x.id === id);
  return p ? `${p.name}: ${p.price} ${p.cadence}` : "";
};

export function llmsTxt(): string {
  const linux = releaseAssets().get("linux") ?? [];
  return `# ${SITE.product}

> ${SITE.tagline} ${SITE.product} runs coding agent CLIs (${ENGINES.slice(0, 3).join(", ")} and others) side by side, in real terminals or as conversations, on your own subscriptions.

- Current release: ${RELEASE.version}, for ${DOWNLOADS.filter((d) => d.available)
    .map((d) => d.name)
    .join(", ")}. ${DOWNLOADS.filter((d) => !d.available)
    .map((d) => d.name)
    .join(" and ")} builds are not available yet.
- The download runs the Free plan on your computer, with no account, and doesn't contact ${SITE.product}'s servers.
- Engines it detects and runs: ${ENGINES.join(", ")}. Each signs in by itself; ${SITE.product} never reads their credentials.
- Prices are in US dollars. ${planLine("free")} (available now). ${planLine("pro")}. ${planLine("team")}. Pro and Team start when ${SITE.product} accounts open.
- Credits pay only for hosted AI features; one credit is $${CREDIT_VALUE_USD.toFixed(3)}. ${CREDIT_ROWS.map((r) => `${r.label}: ${r.credits === 0 ? "free" : `${r.credits} credits ${r.unit}`}`).join("; ")}.

## Docs

${DOCS_PAGES.map((p) => {
  const d = docSource(p.slug);
  return `- [${p.title}](${abs(docsPath(p.slug))}): ${d.description}${notInApp(d) ? " (Not in the app yet.)" : partOfPro(d) ? " (Part of Pro, which starts when accounts open.)" : ""}`;
}).join("\n")}

## Blog

${POSTS.map((p) => `- [${p.title}](${abs(p.path)}): ${p.description}`).join("\n")}
- [All posts](${abs(BLOG_PATH)}) and the [Atom feed](${abs(FEED_PATH)})

## Download

${linux.map((a) => `- [${a.file}](${a.url}): SHA-256 ${a.sha256}`).join("\n")}
- [SHA256SUMS.txt](${abs(CHECKSUMS_URL)}): checksums for every build
- [Download page](${abs("/download/")})

## Optional

- [Full docs as one file](${abs("/llms-full.txt")})
- [Site info as JSON](${abs(SITE_INFO_PATH)}): name, version, platforms and pricing
- [OpenAPI 3.1 document](${abs("/openapi.json")})
- [Pricing](${abs("/pricing/")})
- [Changelog](${abs("/changelog/")})
- [Privacy](${abs("/privacy/")})
`;
}

export function llmsFullTxt(): string {
  const pages = DOCS_PAGES.map((p) => {
    const d = docSource(p.slug);
    return `# ${d.title}\n\nSource: ${abs(docsPath(p.slug))}\n\n> ${d.description}\n\n${toMarkdown(d.body)}`;
  });
  return `${llmsTxt()}\n---\n\n${pages.join("\n\n---\n\n")}\n`;
}

/** The OpenAPI 3.1 document for the site's public endpoint. */
export function openApi() {
  const { $schema: _, ...schema } = z.toJSONSchema(SiteInfoSchema, { target: "draft-2020-12" });
  return {
    openapi: "3.1.0",
    info: {
      title: `${SITE.product} site`,
      version: RELEASE.version,
      description: `Public, read-only endpoints of ${SITE.url}. No authentication.`,
    },
    servers: [{ url: SITE.url }],
    paths: {
      [SITE_INFO_PATH]: {
        get: {
          operationId: "getSiteInfo",
          summary: "Name, version, platforms and pricing",
          responses: {
            "308": {
              description: `Every path on the site ends in a slash: this redirects to ${SITE_INFO_PATH}/, which answers 200.`,
            },
            "200": {
              description: "Site info",
              content: { "application/json": { schema: { $ref: "#/components/schemas/SiteInfo" } } },
            },
          },
        },
      },
    },
    components: { schemas: { SiteInfo: schema } },
  };
}
