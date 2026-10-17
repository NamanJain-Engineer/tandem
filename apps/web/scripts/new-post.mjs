// `pnpm --filter web new-post "Your title"` (F-WB-06, docs/BLOG.md §3):
// writes content/blog/<slug>.mdx with its frontmatter filled in, as a
// draft, so it shows only in `pnpm --filter web dev` until you publish it.
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

/** "What's new in 0.8.0" → "whats-new-in-0-8-0". */
export function slugify(title) {
  return title
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/['’]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** The new post's file name and text. `date` is YYYY-MM-DD. */
export function scaffold(title, date, author = "Naman Jain") {
  const slug = slugify(title);
  if (!slug) throw new Error("The title needs at least one letter or digit.");
  const yaml = (s) => JSON.stringify(s);
  const text = `---
title: ${yaml(title)}
description: One or two sentences about the post. They show on the blog, in the feed and when the post is shared.
date: ${date}
author: ${yaml(author)}
tags: [product]
draft: true
---

Start with what the reader gets from this post.

## A first section

Write in Markdown. Headings build the post's "On this page" list.
`;
  return { slug, file: `${slug}.mdx`, text };
}

function main() {
  const title = process.argv.slice(2).join(" ").trim();
  if (!title) {
    process.stderr.write('Usage: pnpm --filter web new-post "Your title"\n');
    process.exit(1);
  }
  const date = new Date().toISOString().slice(0, 10);
  const { slug, file, text } = scaffold(title, date);
  const dir = join(import.meta.dirname, "..", "content", "blog");
  const path = join(dir, file);
  if (existsSync(path)) {
    process.stderr.write(`content/blog/${file} already exists.\n`);
    process.exit(1);
  }
  mkdirSync(dir, { recursive: true });
  writeFileSync(path, text);
  process.stdout.write(
    `Wrote apps/web/content/blog/${file} (a draft).\nPreview: pnpm --filter web dev, then http://localhost:3000/blog/${slug}/\nTo publish, delete "draft: true" and merge (docs/BLOG.md).\n`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
