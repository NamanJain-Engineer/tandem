import { Avatar } from "@tandem/ui/components/Avatar";
import { buttonClass } from "@tandem/ui/components/Button";
import { Mail, Rss } from "lucide-react";
import { cx } from "../../lib/cx";
import { BLOG_PATH, FEED_PATH, formatDate, tagPath, type Cover, type Post } from "../../lib/blog/posts";
import { SITE } from "../../lib/site";
import { Icon } from "../site/Icon";

// The blog's building blocks (F-WB-06). Server components only: the blog
// ships no script beyond a post's JSON-LD (D-1852). Dates appear only inside
// <time datetime> (D-1851).

/** "15 October 2026", machine-readable too. */
export function PostDate({ date, className }: { date: string; className?: string }) {
  return (
    <time dateTime={date} className={className}>
      {formatDate(date)}
    </time>
  );
}

const Dot = () => (
  <span aria-hidden="true" className="text-fg-subtle">
    ·
  </span>
);

/** The author's monogram and name, the date and the reading time. */
export function PostMeta({ post, avatar = true, className }: { post: Post; avatar?: boolean; className?: string }) {
  return (
    <div className={cx("flex flex-wrap items-center gap-x-2.5 gap-y-1 text-copy-sm text-fg-muted", className)}>
      {avatar && (
        <span aria-hidden="true" className="contents">
          <Avatar name={post.author} id={post.author} size={24} className="mr-0.5" />
        </span>
      )}
      <span className="font-medium text-fg">{post.author}</span>
      <Dot />
      <PostDate date={post.date} />
      <Dot />
      <span>{post.readingMinutes} min read</span>
    </div>
  );
}

/** A post's tags, each linking to its page. */
export function TagList({ tags, className }: { tags: readonly string[]; className?: string }) {
  return (
    <ul aria-label="Tags" className={cx("relative z-10 flex flex-wrap gap-1.5", className)}>
      {tags.map((t) => (
        <li key={t}>
          <a
            href={tagPath(t)}
            className="inline-flex min-h-7 items-center rounded-full border border-border px-2.5 font-mono text-eyebrow uppercase tracking-[0.06em] text-fg-muted transition-colors duration-(--dur-instant) hover:border-border-strong hover:text-fg"
          >
            {t}
          </a>
        </li>
      ))}
    </ul>
  );
}

/** The tag filter above a list: All, then every tag in use. */
export function TagFilter({ tags, current }: { tags: ReadonlyArray<{ tag: string; count: number }>; current?: string }) {
  const item = (href: string, label: string, count: number | null, on: boolean) => (
    <li key={href}>
      <a
        href={href}
        {...(on ? { "aria-current": "page" as const } : {})}
        className="inline-flex min-h-9 items-center gap-2 rounded-full border border-border px-3.5 text-copy-sm text-fg-muted transition-colors duration-(--dur-instant) hover:border-border-strong hover:text-fg aria-[current=page]:border-accent aria-[current=page]:text-fg"
      >
        {label}
        {count !== null && <span className="font-mono text-caption text-fg-subtle">{count}</span>}
      </a>
    </li>
  );
  return (
    <nav aria-label="Tags">
      <ul className="flex flex-wrap gap-2">
        {item(BLOG_PATH, "All posts", null, current === undefined)}
        {tags.map((t) => item(tagPath(t.tag), t.tag, t.count, current === t.tag))}
      </ul>
    </nav>
  );
}

/** A post in a list: date on the left, then title, description and tags.
 * The whole row is one link (the title's, stretched over it). */
export function PostRow({ post, headingLevel = 3 }: { post: Post; headingLevel?: 2 | 3 }) {
  const H = headingLevel === 2 ? "h2" : "h3";
  return (
    <li className="group relative grid gap-3 border-t border-border py-8 md:grid-cols-12 md:gap-6">
      <div className="font-mono text-caption text-fg-muted md:col-span-3 md:pt-1.5">
        <PostDate date={post.date} />
      </div>
      <div className="flex flex-col gap-3 md:col-span-9">
        <H className="text-display-md text-balance text-fg transition-colors duration-(--dur-instant) group-hover:text-accent">
          <a href={post.path} className="after:absolute after:inset-0 focus-visible:outline-none">
            {post.title}
          </a>
        </H>
        <p className="max-w-measure text-copy text-pretty text-fg-muted">{post.description}</p>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <TagList tags={post.tags} />
          <span className="text-caption text-fg-muted">{post.readingMinutes} min read</span>
        </div>
      </div>
    </li>
  );
}

/** A post's cover: a fixed-size <picture> (AVIF, WebP, then the file) on the
 * dark stage, with its caption. It's at the top, so it loads first. */
export function CoverFigure({ cover, eager = true, className }: { cover: Cover; eager?: boolean; className?: string }) {
  const base = cover.src.replace(/\.(png|jpe?g)$/, "");
  const modern = base !== cover.src;
  return (
    <figure className={cx("flex flex-col gap-3", className)}>
      <div data-stage="" className="overflow-hidden rounded-lg bg-stage shadow-stage">
        <picture>
          {modern && <source type="image/avif" srcSet={`${base}.avif`} />}
          {modern && <source type="image/webp" srcSet={`${base}.webp`} />}
          <img
            src={cover.src}
            width={cover.width}
            height={cover.height}
            alt={cover.alt}
            loading={eager ? "eager" : "lazy"}
            decoding="async"
            {...(eager ? { fetchPriority: "high" as const } : {})}
            className="block h-auto w-full"
          />
        </picture>
      </div>
      {cover.caption && <figcaption className="text-caption text-pretty text-fg-muted">{cover.caption}</figcaption>}
    </figure>
  );
}

/** How to follow the blog: the Atom feed, or an email. No newsletter form:
 * it would hand visitors' addresses to a third party. */
export function Subscribe({ className }: { className?: string }) {
  return (
    <aside
      aria-labelledby="subscribe-title"
      className={cx(
        "relative isolate overflow-hidden rounded-lg border border-border bg-card p-8 md:p-10",
        className,
      )}
    >
      <div aria-hidden="true" className="bg-site-grid absolute inset-0 -z-10 [mask-image:linear-gradient(to_left,black,transparent_70%)]" />
      <div className="flex flex-col gap-6 md:flex-row md:items-end md:justify-between">
        <div className="flex max-w-measure flex-col gap-3">
          <h2 id="subscribe-title" className="text-display-md text-fg">
            Follow along
          </h2>
          <p className="text-copy text-pretty text-fg-muted">
            New posts arrive in the Atom feed, in any feed reader. Questions or ideas for a post? Write to{" "}
            <a href={`mailto:${SITE.contactEmail}`} className="text-accent">
              {SITE.contactEmail}
            </a>
            .
          </p>
        </div>
        <div className="flex shrink-0 flex-wrap gap-3">
          <a href={FEED_PATH} className={buttonClass("outline", "lg")}>
            <Icon icon={Rss} size={16} />
            Atom feed
          </a>
          <a href={`mailto:${SITE.contactEmail}?subject=${encodeURIComponent(`${SITE.product} blog`)}`} className={buttonClass("ghost", "lg")}>
            <Icon icon={Mail} size={16} />
            Email us
          </a>
        </div>
      </div>
    </aside>
  );
}
