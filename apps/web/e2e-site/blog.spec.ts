import { expect, test } from "@playwright/test";
import { BLOG_PATH, FEED_PATH, POSTS, blogPages } from "../lib/blog/posts";

// F-WB-06: the blog as the export serves it (out/, after strip-runtime).

const LD = '<script type="application/ld+json">';
const YEAR = /\b(19|20)\d{2}\b/;
/** D-1851: the only years on a blog page are in <time datetime> and the JSON-LD dates. */
const withoutDates = (h: string) =>
  h
    .replace(/<time datetime="\d{4}-\d{2}-\d{2}"[^>]*>[^<]*<\/time>/gi, "")
    .replace(/"date(Published|Modified)":"\d{4}-\d{2}-\d{2}"/g, "");

test("F-WB-06 AC6 blog pages carry no script but a post's JSON-LD, and no year outside their dates", async ({ request }) => {
  for (const path of blogPages()) {
    const html = (await (await request.get(path)).text()).replace(/<!-- -->/g, "");
    const scripts = [...html.matchAll(/<script[^>]*>/g)].map((m) => m[0]);
    const isPost = POSTS.some((p) => p.path === path);
    expect(scripts, path).toEqual(isPost ? [LD] : []);
    expect(withoutDates(html), path).not.toMatch(YEAR);
    expect(html, path).toContain(`<link rel="alternate" type="application/atom+xml" title="Vibezeo blog" href="https://vibezeo.com${FEED_PATH}"/>`);
    expect(html.match(/<h1[\s>]/g)?.length, path).toBe(1);
    expect(html, path).toMatch(/<link rel="canonical" href="https:\/\/vibezeo\.com\/blog\//);
    if (isPost) {
      const ld = /<script type="application\/ld\+json">([\s\S]*?)<\/script>/.exec(html)?.[1] ?? "";
      const data = JSON.parse(ld) as Record<string, unknown>;
      expect(data["@type"], path).toBe("BlogPosting");
      expect(html, path).toContain('<meta property="og:type" content="article"/>');
    }
  }
});

test("F-WB-06 AC5 the Atom feed parses and lists every post", async ({ page, request }) => {
  const res = await request.get(FEED_PATH);
  expect(res.status()).toBe(200);
  const xml = await res.text();
  await page.goto(BLOG_PATH);
  const parsed = await page.evaluate((text) => {
    const doc = new DOMParser().parseFromString(text, "application/xml");
    return {
      error: doc.querySelector("parsererror")?.textContent ?? null,
      root: doc.documentElement.namespaceURI,
      entries: [...doc.querySelectorAll("entry > id")].map((e) => e.textContent),
    };
  }, xml);
  expect(parsed.error).toBeNull();
  expect(parsed.root).toBe("http://www.w3.org/2005/Atom");
  expect(parsed.entries).toEqual(POSTS.map((p) => `https://vibezeo.com${p.path}`));
});

test("F-WB-06 AC3 a post renders its body: anchored headings, highlighted code, a fixed-size cover", async ({ page }) => {
  for (const post of POSTS) {
    await page.goto(post.path);
    await expect(page.locator("h1")).toHaveText(post.title);
    await expect(page.locator(`time[datetime="${post.date}"]`).first()).toBeVisible();
    const h2 = page.locator("[data-post-body] h2[id]").first();
    await expect(h2).toBeVisible();
    const id = await h2.getAttribute("id");
    await expect(h2.locator("a")).toHaveAttribute("href", `#${id}`);
    if (post.cover) {
      const img = page.locator(`img[src="${post.cover.src}"]`);
      await expect(img).toHaveAttribute("width", String(post.cover.width));
      await expect(img).toHaveAttribute("height", String(post.cover.height));
    }
  }
  // Code is highlighted at build time, in the visitor's theme.
  const withCode = POSTS.find((p) => p.tags.includes("release"));
  if (withCode) {
    await page.goto(withCode.path);
    const pre = page.locator("[data-post-body] pre").first();
    if (await pre.count()) await expect(pre.locator("span[style*='--shiki-dark']").first()).toBeAttached();
  }
});

test("F-WB-06 the header and footer lead to the blog, and its pages mark it current", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator(`header nav a[href="${BLOG_PATH}"]`).first()).toHaveText("Blog");
  await expect(page.locator(`footer a[href="${BLOG_PATH}"]`)).toHaveText("Blog");
  await page.goto(BLOG_PATH);
  await expect(page.locator(`header nav a[href="${BLOG_PATH}"][aria-current="page"]`).first()).toBeVisible();
});
