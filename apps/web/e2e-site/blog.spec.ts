import { expect, test } from "@playwright/test";
import { BLOG_PATH, FEED_PATH, POSTS, blogPages, ogImagePath } from "../lib/blog/posts";

// F-WB-06: the blog as the export serves it (out/, after strip-runtime).

const LD = '<script type="application/ld+json">';
const SCRIPT = '<script src="/blog/blog.js" defer="">';
const YEAR = /\b(19|20)\d{2}\b/;
/** D-1851: the only years on a blog page are in <time datetime> and the JSON-LD dates. */
const withoutDates = (h: string) =>
  h
    .replace(/<time datetime="\d{4}-\d{2}-\d{2}"[^>]*>[^<]*<\/time>/gi, "")
    .replace(/"date(Published|Modified)":"\d{4}-\d{2}-\d{2}"/g, "");

test("F-WB-06 AC6 index and tag pages carry no script, a post only its JSON-LD and the Copy script (D-1860), and no year outside their dates", async ({ request }) => {
  for (const path of blogPages()) {
    const html = (await (await request.get(path)).text()).replace(/<!-- -->/g, "");
    const scripts = [...html.matchAll(/<script[^>]*>/g)].map((m) => m[0]);
    const isPost = POSTS.some((p) => p.path === path);
    expect(scripts, path).toEqual(isPost ? [LD, SCRIPT] : []);
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

test("F-WB-06 every post has a share image drawn at build time", async ({ request }) => {
  for (const post of POSTS) {
    const res = await request.get(ogImagePath(post.slug));
    expect(res.status(), post.slug).toBe(200);
    const png = await res.body();
    expect(png.subarray(1, 4).toString(), post.slug).toBe("PNG");
    // 1200×630, read from the PNG header.
    expect([png.readUInt32BE(16), png.readUInt32BE(20)], post.slug).toEqual([1200, 630]);
    const html = await (await request.get(post.path)).text();
    expect(html, post.slug).toContain(`<meta property="og:image" content="https://vibezeo.com${ogImagePath(post.slug)}"/>`);
  }
});

test("F-WB-06 (D-1860) a code block's Copy button appears with the script and copies the code", async ({ browser }) => {
  const withCode = POSTS.find((p) => p.tags.includes("release"));
  test.skip(!withCode, "no post with code");
  const context = await browser.newContext({ permissions: ["clipboard-read", "clipboard-write"] });
  const page = await context.newPage();
  await page.goto(withCode?.path ?? "/");
  const block = page.locator("[data-code]").first();
  const button = block.getByRole("button", { name: "Copy code" });
  await block.hover();
  await expect(button).toBeVisible();
  await button.click();
  await expect(block.locator("[data-copy-status]")).toHaveText("Code copied to the clipboard");
  const copied = await page.evaluate(() => navigator.clipboard.readText());
  expect(copied).toBe((await block.locator("pre").innerText()).replace(/\n$/, ""));
  // Without the script the button stays hidden: nothing that can't work is shown.
  const bare = await browser.newContext({ javaScriptEnabled: false });
  const noJs = await bare.newPage();
  await noJs.goto(withCode?.path ?? "/");
  await expect(noJs.locator("[data-code] button[data-copy]").first()).toBeHidden();
  await bare.close();
  await context.close();
});

test("F-WB-06 (D-1861) the reading progress follows the scroll, and stays still under reduced motion", async ({ browser }) => {
  const post = POSTS[0];
  test.skip(!post, "no posts");
  for (const reducedMotion of ["no-preference", "reduce"] as const) {
    const context = await browser.newContext({ reducedMotion, viewport: { width: 1280, height: 800 } });
    const page = await context.newPage();
    await page.goto(post?.path ?? "/");
    const bar = page.locator("[data-reading-progress]");
    // At the top it's scaled to nothing, so check it's drawn at all.
    if (reducedMotion === "reduce") {
      await expect(bar).toHaveCSS("display", "none");
    } else {
      await expect(bar).toHaveCSS("display", "block");
      const scale = () => bar.evaluate((el) => new DOMMatrix(getComputedStyle(el).transform).a);
      expect(await scale()).toBeLessThan(0.05);
      await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
      await expect.poll(scale).toBeGreaterThan(0.95);
    }
    await context.close();
  }
});
