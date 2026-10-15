// A blog post's frontmatter (F-WB-06). One schema for both readers: the
// fumadocs-mdx collection in source.config.ts, which compiles the bodies, and
// lib/blog/posts.ts, which lists the posts for the index, tags, feed and
// sitemap. Either fails the build on a bad post, naming the file and field.
import { z } from "zod";

/** A post's URL segment: its file name without `.mdx` or `.md`. */
export const SLUG_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
/** Tags are lowercase words or hyphenated phrases; each gets a page. */
export const TAG_RE = SLUG_RE;

/** YAML reads an unquoted `2026-10-15` as a Date; a quoted one stays a
 * string. Both become the same `YYYY-MM-DD` string. */
const date = z.union([z.string(), z.date()]).transform((v, ctx) => {
  const s = v instanceof Date ? (Number.isNaN(v.getTime()) ? "" : v.toISOString().slice(0, 10)) : v.trim();
  const ok = /^\d{4}-\d{2}-\d{2}$/.test(s) && new Date(`${s}T00:00:00Z`).toISOString().startsWith(s);
  if (!ok) {
    ctx.addIssue({ code: "custom", message: "expected a real date written YYYY-MM-DD" });
    return z.NEVER;
  }
  return s;
});

const text = (what: string) => z.string({ error: `expected ${what}` }).trim().min(1, `expected ${what}`);

export const blogFrontmatter = z
  .object({
    title: text("a title"),
    description: text("one or two sentences"),
    date,
    author: text("the author's name"),
    tags: z
      .array(z.string().regex(TAG_RE, "tags are lowercase words or hyphenated phrases"))
      .min(1, "expected at least one tag"),
    cover: z
      .object({
        src: z.string().regex(/^\/blog\/[a-z0-9-]+\/[^/]+\.(png|jpe?g|webp|avif)$/, "expected /blog/<slug>/<file>.png"),
        alt: text("the image's alt text"),
        width: z.number().int().positive(),
        height: z.number().int().positive(),
        caption: z.string().trim().min(1).optional(),
        /** A capture of the app names the build it shows (D-1834), e.g. 0.7.2. */
        build: z.string().regex(/^\d+\.\d+\.\d+$/, "expected a version like 0.7.2").optional(),
      })
      .strict()
      .optional(),
    draft: z.boolean().optional(),
  })
  .strict();

export type BlogFrontmatter = z.output<typeof blogFrontmatter>;
