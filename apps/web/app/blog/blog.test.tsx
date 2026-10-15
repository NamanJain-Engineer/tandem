import { mkdtempSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { BlogIndex } from "../../components/blog/BlogIndex";
import { BLOG_MDX } from "../../components/blog/mdx";
import { PostLayout } from "../../components/blog/PostLayout";
import { TagIndex } from "../../components/blog/TagIndex";
import { FOOTER, NAV } from "../../components/site/nav";
import {
  BLOG_PATH,
  FEED_PATH,
  POSTS,
  atomFeed,
  blogPages,
  formatDate,
  loadPosts,
  neighbours,
  parsePost,
  ogImagePath,
  postJsonLd,
  postPath,
  related,
  titleTransition,
  tagPath,
  tagsOf,
  type Post,
} from "../../lib/blog/posts";
import { llmsTxt } from "../../lib/discovery";
import { SCREENS } from "../../lib/media";
import { RELEASE } from "../../lib/release";
import { SITE, sitemapXml } from "../../lib/site";
import { scaffold, slugify } from "../../scripts/new-post.mjs";
import { allowedScripts } from "../../scripts/script-allowlist.mjs";
import BlogPage, { metadata as indexMetadata } from "./page.site";
import TagPage, { generateMetadata as tagMetadata, generateStaticParams as tagParams } from "./tags/[tag]/page.site";

// F-WB-06: the blog. Posts are files under content/blog; these tests use the
// real posts and fixtures written to a temporary folder.

const html = (el: ReactElement) => renderToStaticMarkup(el);
const YEAR = /\b(19|20)\d{2}\b/;
/** D-1851: a blog page may show a year only in a <time datetime> element
 * and the JSON-LD's date fields. */
const withoutDates = (h: string) =>
  h
    .replace(/<time datetime="\d{4}-\d{2}-\d{2}"[^>]*>[^<]*<\/time>/gi, "")
    .replace(/"date(Published|Modified)":"\d{4}-\d{2}-\d{2}"/g, "");

const front = (over: Record<string, string> = {}) => {
  const fields: Record<string, string> = {
    title: "A post",
    description: "What it says.",
    date: "2026-10-15",
    author: "Naman Jain",
    tags: "[release]",
    ...over,
  };
  const yaml = Object.entries(fields)
    .filter(([, v]) => v !== "")
    .map(([k, v]) => `${k}: ${v}`)
    .join("\n");
  return `---\n${yaml}\n---\n\nSome words in the body.\n`;
};

const fixture = (files: Record<string, string>) => {
  const dir = mkdtempSync(join(tmpdir(), "blog-"));
  const content = join(dir, "content");
  mkdirSync(content);
  mkdirSync(join(dir, "public"));
  for (const [name, raw] of Object.entries(files)) writeFileSync(join(content, name), raw);
  return { content, publicDir: join(dir, "public") };
};

const post = (slug: string, date: string, tags: string[], extra: Partial<Post> = {}): Post => ({
  slug,
  file: `${slug}.mdx`,
  path: postPath(slug),
  title: `Title of ${slug}`,
  description: `About ${slug}.`,
  date,
  author: "Naman Jain",
  tags,
  draft: false,
  readingMinutes: 3,
  ...extra,
});

describe("blog (F-WB-06)", () => {
  it("F-WB-06 AC1 a bad frontmatter fails with the file and the field named", () => {
    const pub = mkdtempSync(join(tmpdir(), "blog-pub-"));
    const bad: Array<[Record<string, string>, string]> = [
      [{ title: "" }, "title"],
      [{ description: "" }, "description"],
      [{ date: "15 October" }, "date"],
      [{ date: "2026-02-30" }, "date"],
      [{ author: "" }, "author"],
      [{ tags: "[Release]" }, "tags"],
      [{ tags: "[]" }, "tags"],
      [{ titel: "Typo" }, "titel"],
      [{ draft: '"yes"' }, "draft"],
    ];
    for (const [over, field] of bad) {
      expect(() => parsePost("my-post.mdx", front(over), pub), field).toThrow(
        new RegExp(`content/blog/my-post\\.mdx.*${field}`),
      );
    }
    expect(() => parsePost("my-post.mdx", "No frontmatter here", pub)).toThrow(/content\/blog\/my-post\.mdx.*frontmatter/);
    expect(() => parsePost("My Post.mdx", front(), pub)).toThrow(/content\/blog\/My Post\.mdx.*file name/);
    // A cover must exist under public/blog/<slug>/.
    const cover = "{ src: /blog/my-post/cover.png, alt: The app, width: 1440, height: 900 }";
    expect(() => parsePost("my-post.mdx", front({ cover }), pub)).toThrow(/content\/blog\/my-post\.mdx.*cover.*\/blog\/my-post\/cover\.png/);
    mkdirSync(join(pub, "blog", "my-post"), { recursive: true });
    writeFileSync(join(pub, "blog", "my-post", "cover.png"), "");
    expect(parsePost("my-post.mdx", front({ cover }), pub).cover?.width).toBe(1440);
    const elsewhere = "{ src: /blog/other/cover.png, alt: The app, width: 1440, height: 900 }";
    expect(() => parsePost("my-post.mdx", front({ cover: elsewhere }), pub)).toThrow(/cover.*\/blog\/my-post\//);
  });

  it("F-WB-06 AC1 a good post parses: unquoted dates, reading time, Markdown files too", () => {
    const pub = mkdtempSync(join(tmpdir(), "blog-pub-"));
    const p = parsePost("hello-world.md", front({ date: "2026-10-01" }) + "word ".repeat(700), pub);
    expect(p).toMatchObject({ slug: "hello-world", path: "/blog/hello-world/", date: "2026-10-01", draft: false });
    expect(p.readingMinutes).toBe(4);
    expect(parsePost("a.mdx", front({ date: '"2026-10-01"' }), pub).date).toBe("2026-10-01");
    expect(parsePost("a.mdx", front(), pub).readingMinutes).toBe(1);
  });

  it("F-WB-06 AC2 posts are newest first, drafts only on request, and two files can't share a slug", () => {
    const { content, publicDir } = fixture({
      "older.mdx": front({ date: "2026-09-01" }),
      "newer.mdx": front({ date: "2026-10-01" }),
      "draft.mdx": front({ date: "2026-10-09", draft: "true" }),
      "notes.txt": "not a post",
    });
    expect(loadPosts(content, publicDir).map((p) => p.slug)).toEqual(["newer", "older"]);
    expect(loadPosts(content, publicDir, { drafts: true }).map((p) => p.slug)).toEqual(["draft", "newer", "older"]);
    // Same day: by title, so the order never depends on the file system.
    const same = fixture({ "b.mdx": front({ title: "Beta" }), "a.mdx": front({ title: "Alpha" }) });
    expect(loadPosts(same.content, same.publicDir).map((p) => p.title)).toEqual(["Alpha", "Beta"]);
    const dup = fixture({ "same.mdx": front(), "same.md": front() });
    expect(() => loadPosts(dup.content, dup.publicDir)).toThrow(/same\.md.*same\.mdx|same\.mdx.*same\.md/);
    // No folder yet means no posts, not a failed build.
    expect(loadPosts(join(content, "missing"), publicDir)).toEqual([]);
  });

  it("F-WB-06 AC2 the index features the newest post above the rest, and says so when there are none", () => {
    const posts = [post("c", "2026-10-03", ["release"]), post("b", "2026-10-02", ["linux"]), post("a", "2026-10-01", ["release"])];
    const h = html(<BlogIndex posts={posts} />);
    expect(h.match(/<h1[\s>]/g)?.length).toBe(1);
    const featured = /<article[^>]*data-featured=""[\s\S]*?<\/article>/.exec(h)?.[0] ?? "";
    expect(featured).toContain(`href="${postPath("c")}"`);
    const order = posts.map((p) => h.indexOf(`href="${p.path}"`));
    expect(order).toEqual([...order].sort((x, y) => x - y));
    for (const t of tagsOf(posts)) expect(h).toContain(`href="${tagPath(t.tag)}"`);
    expect(h).toContain(`href="${FEED_PATH}"`);
    expect(h).toContain(`mailto:${SITE.contactEmail}`);
    expect(h).not.toContain("data-empty");

    const empty = html(<BlogIndex posts={[]} />);
    expect(empty).toContain("data-empty");
    expect(empty).toContain("No posts yet");
    expect(empty.match(/<h1[\s>]/g)?.length).toBe(1);
    expect(empty).toContain(`href="${FEED_PATH}"`);
    expect(empty).not.toContain("data-featured");
  });

  it("F-WB-06 AC3 a post page has its title, author, date, reading time, tags, neighbours and a way to subscribe", () => {
    const newer = post("newer", "2026-10-03", ["release"]);
    const p = post("this-one", "2026-10-02", ["release", "linux"], { readingMinutes: 5 });
    const older = post("older", "2026-10-01", ["linux"]);
    const toc = [
      { title: "First part", url: "#first-part", depth: 2 },
      { title: "Second part", url: "#second-part", depth: 2 },
    ];
    const h = html(
      <PostLayout post={p} newer={newer} older={older} toc={toc}>
        <p>Body text.</p>
      </PostLayout>,
    );
    expect(h.match(/<h1[\s>]/g)?.length).toBe(1);
    expect(h).toContain(`>${p.title}</h1>`);
    expect(h).toContain(p.description);
    expect(h).toContain(p.author);
    expect(h).toContain(`<time dateTime="${p.date}"`);
    expect(h).toContain(formatDate(p.date));
    expect(h).toContain("5 min read");
    for (const t of p.tags) expect(h).toContain(`href="${tagPath(t)}"`);
    expect(h).toContain(`href="${newer.path}"`);
    expect(h).toContain(`href="${older.path}"`);
    expect(h).toContain(`href="${BLOG_PATH}"`);
    expect(h).toContain('href="#first-part"');
    expect(h).toContain(`href="${FEED_PATH}"`);
    expect(h).toContain("Body text.");
    // No cover: no image, and the title block still carries the page.
    expect(h).not.toMatch(/<img[\s>]/);
    // The first post has no newer neighbour, the last no older one.
    const first = html(<PostLayout post={newer} older={p} toc={[]}>x</PostLayout>);
    expect(first).toContain(`href="${p.path}"`);
    expect(first).not.toContain("On this page");
  });

  it("F-WB-06 AC3 a cover is a fixed-size picture with AVIF and WebP, loaded first, with its caption", () => {
    const p = post("with-cover", "2026-10-02", ["release"], {
      cover: { src: "/blog/with-cover/shot.png", alt: "The app", width: 1440, height: 900, caption: "Captured from build 0.1.0." },
    });
    const h = html(<PostLayout post={p} toc={[]}>x</PostLayout>);
    expect(h).toMatch(/<source type="image\/avif" srcSet="\/blog\/with-cover\/shot\.avif"/);
    expect(h).toMatch(/<source type="image\/webp" srcSet="\/blog\/with-cover\/shot\.webp"/);
    expect(h).toMatch(/<img[^>]*src="\/blog\/with-cover\/shot\.png"[^>]*width="1440"[^>]*height="900"/);
    expect(h).toContain('alt="The app"');
    expect(h).toContain("Captured from build 0.1.0.");
    // A capture of the app names its build (D-1834) and offers its 2x files (D-1838).
    const shot = post("shot", "2026-10-02", ["release"], {
      cover: { src: "/blog/shot/app.png", alt: "The app", width: 1440, height: 900, build: "0.7.2", retina: true },
    });
    const s = html(<PostLayout post={shot} toc={[]}>x</PostLayout>);
    expect(s).toContain("Vibezeo 0.7.2 · Linux");
    expect(s).toContain('srcSet="/blog/shot/app.avif 1440w, /blog/shot/app@2x.avif 2880w"');
    // Phones take an 800 px copy when there is one (D-1864).
    const phone = post("shot", "2026-10-02", ["release"], {
      cover: { src: "/blog/shot/app.png", alt: "The app", width: 1440, height: 900, retina: true, narrow: true },
    });
    expect(html(<PostLayout post={phone} toc={[]}>x</PostLayout>)).toContain(
      'srcSet="/blog/shot/app-800.avif 800w, /blog/shot/app.avif 1440w, /blog/shot/app@2x.avif 2880w"',
    );
    expect(s).toContain('data-build="0.7.2"');
  });

  it("F-WB-06 a cover's build is a version, and its 2x files are found beside it", () => {
    const pub = mkdtempSync(join(tmpdir(), "blog-pub-"));
    mkdirSync(join(pub, "blog", "my-post"), { recursive: true });
    for (const f of ["cover.png", "cover@2x.avif", "cover@2x.webp", "cover-800.avif", "cover-800.webp"]) writeFileSync(join(pub, "blog", "my-post", f), "");
    const cover = (build: string) => `{ src: /blog/my-post/cover.png, alt: The app, width: 1440, height: 900, build: ${build} }`;
    expect(parsePost("my-post.mdx", front({ cover: cover("0.7.2") }), pub).cover).toMatchObject({ build: "0.7.2", retina: true, narrow: true });
    expect(() => parsePost("my-post.mdx", front({ cover: cover("latest") }), pub)).toThrow(/cover\.build/);
    // The intro's cover is the real capture the home page shows, labelled with its build.
    const intro = POSTS.find((p) => p.slug === "introducing-vibezeo");
    expect(intro?.cover).toMatchObject({ build: SCREENS.codeMode.build, retina: true, narrow: true });
  });

  it("F-WB-06 AC4 a tag page lists exactly the posts with that tag", async () => {
    const posts = [post("c", "2026-10-03", ["release"]), post("b", "2026-10-02", ["linux"]), post("a", "2026-10-01", ["release"])];
    const h = html(<TagIndex tag="release" posts={posts} />);
    expect(h).toContain(`href="${postPath("c")}"`);
    expect(h).toContain(`href="${postPath("a")}"`);
    expect(h).not.toContain(`href="${postPath("b")}"`);
    expect(h.match(/<h1[\s>]/g)?.length).toBe(1);
    // The real tag pages: one per tag in use, each with its canonical URL.
    expect((tagParams()).map((p) => p.tag).sort()).toEqual(tagsOf(POSTS).map((t) => t.tag).sort());
    for (const { tag } of tagParams()) {
      const meta = await tagMetadata({ params: Promise.resolve({ tag }) });
      expect(meta.alternates?.canonical).toBe(tagPath(tag));
      const page = html(await TagPage({ params: Promise.resolve({ tag }) }));
      for (const p of POSTS) expect(page.includes(`href="${p.path}"`), `${tag} ${p.slug}`).toBe(p.tags.includes(tag));
    }
  });

  it("F-WB-06 AC5 the Atom feed has one entry per published post, and every blog page links it", () => {
    const posts = [post("b", "2026-10-02", ["linux"], { title: "Fish & <chips>" }), post("a", "2026-10-01", ["release"])];
    const xml = atomFeed(posts);
    expect(xml.startsWith('<?xml version="1.0" encoding="utf-8"?>\n<feed xmlns="http://www.w3.org/2005/Atom">')).toBe(true);
    expect(xml.trimEnd().endsWith("</feed>")).toBe(true);
    expect(xml.match(/<entry>/g)?.length).toBe(2);
    expect(xml).toContain(`<id>${SITE.url}${postPath("b")}</id>`);
    expect(xml).toContain(`<link rel="alternate" type="text/html" href="${SITE.url}${postPath("b")}"/>`);
    expect(xml).toContain(`<link rel="self" type="application/atom+xml" href="${SITE.url}${FEED_PATH}"/>`);
    expect(xml).toContain("<updated>2026-10-02T00:00:00Z</updated>");
    expect(xml).toContain("Fish &amp; &lt;chips&gt;");
    expect(xml).not.toContain("<chips>");
    expect(atomFeed([]).match(/<entry>/g)).toBeNull();
    // The real feed has every post, and no draft.
    expect(atomFeed(POSTS).match(/<entry>/g)?.length).toBe(POSTS.length);
    for (const p of POSTS) expect(p.draft).toBe(false);
    // <link rel="alternate" type="application/atom+xml"> on the index and tag pages (Next metadata).
    const feedLink = { "application/atom+xml": [{ url: FEED_PATH, title: `${SITE.product} blog` }] };
    expect(indexMetadata.alternates?.types).toEqual(feedLink);
    expect(indexMetadata.alternates?.canonical).toBe(BLOG_PATH);
  });

  it("F-WB-06 AC6 (D-1851) blog pages show a year only inside <time datetime> and the JSON-LD dates", async () => {
    expect(withoutDates(html(<BlogIndex posts={[]} />))).not.toMatch(YEAR);
    const pages = [html(<BlogPage />)];
    for (const { tag } of tagParams()) pages.push(html(await TagPage({ params: Promise.resolve({ tag }) })));
    for (const p of POSTS) {
      const { newer, older } = neighbours(POSTS, p.slug);
      pages.push(html(<PostLayout post={p} newer={newer} older={older} toc={[]}>x</PostLayout>));
      pages.push(JSON.stringify(postJsonLd(p)));
    }
    for (const h of pages) {
      expect(h).toMatch(YEAR); // the dates are there…
      expect(withoutDates(h)).not.toMatch(YEAR); // …and only there
    }
    // A year in a post's own text fails here, before it reaches a page.
    for (const p of POSTS) {
      const body = readFileSync(join(import.meta.dirname, "../../content/blog", p.file), "utf8").replace(/^---[\s\S]*?\n---\n/, "");
      expect(body, p.file).not.toMatch(YEAR);
      expect(`${p.title} ${p.description} ${p.cover?.caption ?? ""} ${p.cover?.alt ?? ""}`, p.file).not.toMatch(YEAR);
    }
  });

  it("F-WB-06 AC6 (D-1852) the runtime strip allows a JSON-LD block on post pages only", () => {
    const ld = '<script type="application/ld+json">';
    expect(allowedScripts("index.html")).toEqual(['<script src="/demo/embed.js" defer="">']);
    expect(allowedScripts("download/index.html")).toEqual(['<script src="/download/detect.js" defer="">']);
    expect(allowedScripts("blog/introducing-vibezeo/index.html")).toEqual([ld, '<script src="/blog/blog.js" defer="">']);
    for (const page of ["blog/index.html", "blog/tags/release/index.html", "pricing/index.html", "changelog/0.7.2/index.html"])
      expect(allowedScripts(page), page).toEqual([]);
    // JSON-LD is data: a BlogPosting whose dates are the post's.
    const p = POSTS[0];
    expect(p).toBeDefined();
    if (!p) return;
    const data = postJsonLd(p);
    expect(data).toMatchObject({
      "@context": "https://schema.org",
      "@type": "BlogPosting",
      headline: p.title,
      datePublished: p.date,
      url: `${SITE.url}${p.path}`,
      author: { "@type": "Person", name: p.author },
    });
  });

  it("F-WB-06 AC7 the header, footer, sitemap and llms.txt link the blog", () => {
    expect(NAV.map((n) => n.href)).toContain(BLOG_PATH);
    expect(FOOTER.flatMap((c) => c.links.map((l) => l.href))).toContain(BLOG_PATH);
    const map = sitemapXml();
    for (const p of blogPages()) expect(map, p).toContain(`<loc>${SITE.url}${p}</loc>`);
    expect(blogPages()).toEqual([
      BLOG_PATH,
      ...POSTS.map((p) => p.path),
      ...tagsOf(POSTS).map((t) => tagPath(t.tag)),
    ]);
    const llms = llmsTxt();
    expect(llms).toContain(`${SITE.url}${BLOG_PATH}`);
    expect(llms).toContain(`${SITE.url}${FEED_PATH}`);
    for (const p of POSTS) expect(llms, p.slug).toContain(`[${p.title}](${SITE.url}${p.path})`);
  });

  it("F-WB-06 the seed posts are Vibezeo's own, first-party and about the shipped build", () => {
    const dir = join(import.meta.dirname, "../../content/blog");
    const files = readdirSync(dir).filter((f) => /\.mdx?$/.test(f));
    expect(files.sort()).toEqual(["introducing-vibezeo.mdx", `whats-new-in-${RELEASE.version.replace(/\./g, "-")}.mdx`]);
    for (const f of files) {
      const raw = readFileSync(join(dir, f), "utf8");
      expect(raw, f).not.toMatch(/\bTandem\b|Bridge|hello@vibezeo\.com/);
      // First-party only: no embeds, no images or scripts from another origin.
      expect(raw, f).not.toMatch(/<(iframe|script|video|audio|embed|object)[\s>]/i);
      expect(raw, f).not.toMatch(/!\[[^\]]*\]\(https?:|src=["']?(https?:)?\/\//);
    }
    const release = POSTS.find((p) => p.slug === `whats-new-in-${RELEASE.version.replace(/\./g, "-")}`);
    expect(release?.title).toBe(`What's new in ${RELEASE.version}`);
    expect(release?.tags).toContain("release");
    // The release post links its release notes.
    const body = readFileSync(join(dir, release?.file ?? ""), "utf8");
    expect(body).toContain(`/changelog/${RELEASE.version}/`);
    expect(body).toContain("/download/");
  });

  it("F-WB-06 new-post writes a draft whose frontmatter passes the build's check", () => {
    expect(slugify("What's new in 0.8.0")).toBe("whats-new-in-0-8-0");
    expect(slugify("  Hello, World!  ")).toBe("hello-world");
    const pub = mkdtempSync(join(tmpdir(), "blog-pub-"));
    const s = scaffold('A "quoted" title: with a colon', "2026-10-20");
    expect(s.file).toBe("a-quoted-title-with-a-colon.mdx");
    const p = parsePost(s.file, s.text, pub);
    expect(p).toMatchObject({ title: 'A "quoted" title: with a colon', date: "2026-10-20", draft: true });
    expect(() => scaffold("!!!", "2026-10-20")).toThrow();
  });

  it("F-WB-06 related posts share a tag, most shared first, and never repeat the page or its neighbours", () => {
    const posts = [
      post("e", "2026-10-05", ["linux"]),
      post("d", "2026-10-04", ["release", "linux"]),
      post("c", "2026-10-03", ["release", "linux", "dictate"]),
      post("b", "2026-10-02", ["product"]),
      post("a", "2026-10-01", ["release"]),
    ];
    expect(related(posts, "c").map((p) => p.slug)).toEqual(["d", "e", "a"]);
    expect(related(posts, "c", [posts[1] as Post]).map((p) => p.slug)).toEqual(["e", "a"]);
    expect(related(posts, "b")).toEqual([]);
    expect(related(posts, "c", [], 1).map((p) => p.slug)).toEqual(["d"]);
    expect(related(posts, "nope")).toEqual([]);
  });

  it("F-WB-06 a post page reads well: progress, related posts, a share image, and titles that carry across pages", () => {
    const p = post("this-one", "2026-10-02", ["release"]);
    const other = post("other", "2026-09-01", ["release"]);
    const h = html(
      <PostLayout post={p} related={[other]} toc={[]}>
        x
      </PostLayout>,
    );
    expect(h).toContain("data-reading-progress");
    expect(h).toContain('aria-labelledby="related-title"');
    expect(h).toContain(`href="${other.path}"`);
    expect(h).toContain('<script src="/blog/blog.js" defer="">');
    expect(h).toContain(`view-transition-name:${titleTransition(p.slug)}`);
    // The index names the same title, so the browser can morph one into the other.
    const index = html(<BlogIndex posts={[p, other]} />);
    expect(index).toContain(`view-transition-name:${titleTransition(p.slug)}`);
    expect(index).toContain(`view-transition-name:${titleTransition(other.slug)}`);
    expect(ogImagePath(p.slug)).toBe("/blog/this-one/og.png");
    expect(postJsonLd(p).image).toBe(`${SITE.url}/blog/this-one/og.png`);
  });

  it("F-WB-06 code blocks get a copy button only when the script runs, and the script is first-party and stores nothing", () => {
    const Pre = BLOG_MDX.pre;
    const h = html(<Pre>{"echo hi"}</Pre>);
    expect(h).toMatch(/<button[^>]*data-copy=""[^>]*hidden=""/);
    expect(h).toContain('aria-live="polite"');
    const js = readFileSync(join(import.meta.dirname, "../../public/blog/blog.js"), "utf8");
    expect(js).toContain("navigator.clipboard");
    expect(js).not.toMatch(/https?:\/\/|document\.cookie|localStorage|sessionStorage|indexedDB|sendBeacon|fetch\(|XMLHttpRequest|import\(/);
  });

  it("F-WB-06 dates read in full, like the rest of the site", () => {
    expect(formatDate("2026-10-15")).toBe("15 October 2026");
    expect(formatDate("2026-01-01")).toBe("1 January 2026");
  });
});
