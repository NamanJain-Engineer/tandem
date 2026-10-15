// The blog's posts (F-WB-06), read from content/blog at build time: the
// index, the tag pages, the feed, the sitemap and llms.txt list them from
// here. The bodies are compiled by fumadocs-mdx (source.config.ts), which
// checks the same frontmatter schema.
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { CORE_SCHEMA, load } from "js-yaml";
import { SITE } from "../site";
import { SLUG_RE, blogFrontmatter, type BlogFrontmatter } from "./schema";

export type Cover = NonNullable<BlogFrontmatter["cover"]> & {
  /** `<name>@2x.avif` and `.webp` sit beside it (D-1838), for sharp screens. */
  retina?: boolean;
};

export type Post = {
  slug: string;
  /** The file under content/blog, e.g. `introducing-vibezeo.mdx`. */
  file: string;
  path: string;
  title: string;
  description: string;
  /** YYYY-MM-DD. */
  date: string;
  author: string;
  tags: string[];
  cover?: Cover;
  draft: boolean;
  readingMinutes: number;
};

export const BLOG_PATH = "/blog/";
export const FEED_PATH = "/blog/feed.xml";
export const postPath = (slug: string) => `/blog/${slug}/`;
/** A post's share image, drawn at build time (D-1859). */
export const ogImagePath = (slug: string) => `/blog/${slug}/og.png`;
export const OG_SIZE = { width: 1200, height: 630 } as const;
export const tagPath = (tag: string) => `/blog/tags/${tag}/`;

/** Reading speed for "N min read". */
const WORDS_PER_MINUTE = 230;

/** A post's frontmatter and reading time; throws, naming the file and the
 * field, if anything is wrong (AC1). `publicDir` is where covers must exist. */
export function parsePost(file: string, raw: string, publicDir: string): Post {
  const where = `content/blog/${file}`;
  const fail = (why: string): never => {
    throw new Error(`${where}: ${why}. See docs/BLOG.md, "Publishing a post".`);
  };
  const slug = file.replace(/\.mdx?$/, "");
  if (!SLUG_RE.test(slug)) fail(`the file name must be lowercase letters, digits and hyphens (it's the URL)`);
  const m = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/.exec(raw);
  if (!m) return fail("no frontmatter (a block between two --- lines at the top)");
  let data: unknown;
  try {
    // No YAML timestamps: they'd turn 2026-02-30 into 2 March. Dates stay
    // strings and the schema checks them.
    data = load(m[1] ?? "", { schema: CORE_SCHEMA });
  } catch (e) {
    return fail(`the frontmatter isn't valid YAML (${e instanceof Error ? e.message.split("\n")[0] : String(e)})`);
  }
  const parsed = blogFrontmatter.safeParse(data ?? {});
  if (!parsed.success) {
    const issues = parsed.error.issues.map((i) => {
      const keys = i.code === "unrecognized_keys" ? i.keys.join(", ") : "";
      const field = [...i.path.map(String), keys].filter(Boolean).join(".") || "frontmatter";
      return `"${field}": ${i.code === "unrecognized_keys" ? "not a frontmatter field" : i.message}`;
    });
    return fail(issues.join("; "));
  }
  const f = parsed.data;
  if (f.cover) {
    if (!f.cover.src.startsWith(`/blog/${slug}/`)) fail(`"cover.src" must be under /blog/${slug}/`);
    if (!existsSync(join(publicDir, f.cover.src))) fail(`"cover.src": public${f.cover.src} doesn't exist`);
  }
  const words = (m[2] ?? "").split(/\s+/).filter((w) => /\w/.test(w)).length;
  return {
    slug,
    file,
    path: postPath(slug),
    title: f.title,
    description: f.description,
    date: f.date,
    author: f.author,
    tags: f.tags,
    ...(f.cover ? { cover: { ...f.cover, retina: hasRetina(f.cover.src, publicDir) } } : {}),
    draft: f.draft ?? false,
    readingMinutes: Math.max(1, Math.ceil(words / WORDS_PER_MINUTE)),
  };
}

/** Whether AVIF and WebP twice the size sit beside a PNG or JPEG cover. */
function hasRetina(src: string, publicDir: string): boolean {
  const base = src.replace(/\.(png|jpe?g)$/, "");
  if (base === src) return false;
  return ["avif", "webp"].every((f) => existsSync(join(publicDir, `${base}@2x.${f}`)));
}

/** Every post in `dir`, newest first (same day: by title). Drafts only when
 * asked for: `next dev` shows them, the export never does (AC2). */
export function loadPosts(dir: string, publicDir: string, { drafts = false } = {}): Post[] {
  if (!existsSync(dir)) return [];
  const files = readdirSync(dir).filter((f) => /\.mdx?$/.test(f));
  const seen = new Map<string, string>();
  const posts = files.map((file) => {
    const p = parsePost(file, readFileSync(join(dir, file), "utf8"), publicDir);
    const other = seen.get(p.slug);
    if (other) throw new Error(`content/blog/${other} and content/blog/${file} are both /blog/${p.slug}/: rename one.`);
    seen.set(p.slug, file);
    return p;
  });
  return posts
    .filter((p) => drafts || !p.draft)
    .sort((a, b) => b.date.localeCompare(a.date) || a.title.localeCompare(b.title));
}

/** The site's posts. Paths are from apps/web, where Next and vitest run. */
export const POSTS: readonly Post[] = loadPosts(
  join(process.cwd(), "content", "blog"),
  join(process.cwd(), "public"),
  { drafts: process.env.NODE_ENV === "development" },
);

/** Tags in use, most used first, then by name. */
export function tagsOf(posts: readonly Post[]): Array<{ tag: string; count: number }> {
  const counts = new Map<string, number>();
  for (const p of posts) for (const t of p.tags) counts.set(t, (counts.get(t) ?? 0) + 1);
  return [...counts]
    .map(([tag, count]) => ({ tag, count }))
    .sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag));
}

/** The posts either side of `slug` in the newest-first list. */
export function neighbours(posts: readonly Post[], slug: string): { newer?: Post; older?: Post } {
  const i = posts.findIndex((p) => p.slug === slug);
  if (i < 0) return {};
  return { newer: posts[i - 1], older: posts[i + 1] };
}

/** Up to `n` other posts that share a tag with `slug`, most shared tags
 * first, then newest; never the post itself or the ones in `skip` (its
 * neighbours, which the page links already). */
export function related(posts: readonly Post[], slug: string, skip: readonly Post[] = [], n = 3): Post[] {
  const post = posts.find((p) => p.slug === slug);
  if (!post) return [];
  const skipped = new Set([slug, ...skip.map((p) => p.slug)]);
  return posts
    .filter((p) => !skipped.has(p.slug))
    .map((p) => ({ p, shared: p.tags.filter((t) => post.tags.includes(t)).length }))
    .filter((x) => x.shared > 0)
    .sort((a, b) => b.shared - a.shared || b.p.date.localeCompare(a.p.date))
    .slice(0, n)
    .map((x) => x.p);
}

/** The view-transition name shared by a post's title on the index, the tag
 * pages and its own page, so the title carries across the navigation. */
export const titleTransition = (slug: string) => `post-title-${slug}`;

const MONTHS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

/** "15 October 2026": the site's date style (BUILD_LOG, the changelog). Shown
 * only inside <time datetime> (D-1851). */
export function formatDate(date: string): string {
  const [y, m, d] = date.split("-").map(Number);
  return `${d} ${MONTHS[(m ?? 1) - 1]} ${y}`;
}

/** Every exported blog URL, for the sitemap, the routes test and the e2e. */
export function blogPages(posts: readonly Post[] = POSTS): string[] {
  return [BLOG_PATH, ...posts.map((p) => p.path), ...tagsOf(posts).map((t) => tagPath(t.tag))];
}

const xml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** /blog/feed.xml: Atom (RFC 4287), one entry per published post (AC5). */
export function atomFeed(posts: readonly Post[]): string {
  const abs = (p: string) => `${SITE.url}${p}`;
  const stamp = (date: string) => `${date}T00:00:00Z`;
  const updated = posts[0] ? stamp(posts[0].date) : "1970-01-01T00:00:00Z";
  const entries = posts.map(
    (p) => `  <entry>
    <title>${xml(p.title)}</title>
    <id>${abs(p.path)}</id>
    <link rel="alternate" type="text/html" href="${abs(p.path)}"/>
    <published>${stamp(p.date)}</published>
    <updated>${stamp(p.date)}</updated>
    <author><name>${xml(p.author)}</name></author>
${p.tags.map((t) => `    <category term="${xml(t)}"/>`).join("\n")}
    <summary>${xml(p.description)}</summary>
  </entry>`,
  );
  return [
    '<?xml version="1.0" encoding="utf-8"?>',
    '<feed xmlns="http://www.w3.org/2005/Atom">',
    `  <title>${xml(SITE.product)} blog</title>`,
    `  <subtitle>${xml(SITE.tagline)}</subtitle>`,
    `  <id>${abs(BLOG_PATH)}</id>`,
    `  <link rel="self" type="application/atom+xml" href="${abs(FEED_PATH)}"/>`,
    `  <link rel="alternate" type="text/html" href="${abs(BLOG_PATH)}"/>`,
    `  <updated>${updated}</updated>`,
    `  <icon>${abs("/icon.png")}</icon>`,
    ...entries,
    "</feed>",
    "",
  ].join("\n");
}

/** A post's JSON-LD (schema.org BlogPosting): data for search engines, the
 * one script a blog page may carry (D-1852). */
export function postJsonLd(p: Post): Record<string, unknown> {
  return {
    "@context": "https://schema.org",
    "@type": "BlogPosting",
    headline: p.title,
    description: p.description,
    datePublished: p.date,
    url: `${SITE.url}${p.path}`,
    mainEntityOfPage: `${SITE.url}${p.path}`,
    image: `${SITE.url}${ogImagePath(p.slug)}`,
    keywords: p.tags.join(", "),
    author: {
      "@type": "Person",
      name: p.author,
      ...(p.author === SITE.founder.name && SITE.founder.link ? { url: SITE.founder.link } : {}),
    },
    publisher: { "@type": "Organization", name: SITE.company, url: SITE.url, logo: `${SITE.url}/icon.png` },
  };
}

/** The JSON-LD as script text: `<` escaped so a title can't close the tag. */
export const jsonLdText = (p: Post) => JSON.stringify(postJsonLd(p)).replace(/</g, "\\u003c");
