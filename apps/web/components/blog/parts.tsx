import { Avatar } from "@tandem/ui/components/Avatar";
import { siteButton } from "@tandem/ui/components/SiteButton";
import { Mail, Rss } from "lucide-react";
import { cx } from "../../lib/cx";
import { BLOG_PATH, COVER_NARROW, FEED_PATH, formatDate, tagPath, titleTransition, type Cover, type Post } from "../../lib/blog/posts";
import { SITE } from "../../lib/site";
import { Icon } from "../site/Icon";
import { Orbit } from "../site/Orbit";

// The blog's building blocks (F-WB-06), on the site's elite system (D-1830 to
// D-1837): lit surfaces, hairlines, the bezel and the rings' light. Server
// components only; dates appear only inside <time datetime> (D-1851).

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

/** A post's tags, each linking to its page: the site's mono tag, as links. */
export function TagList({ tags, className }: { tags: readonly string[]; className?: string }) {
  return (
    <ul aria-label="Tags" className={cx("relative z-10 flex flex-wrap gap-1.5", className)}>
      {tags.map((t) => (
        <li key={t}>
          <a
            href={tagPath(t)}
            className="inline-flex h-7 items-center rounded-full px-2.5 font-mono text-[0.6875rem] uppercase tracking-[0.1em] text-fg-muted shadow-[inset_0_0_0_1px_var(--site-line-strong)] transition-[color,box-shadow] duration-(--dur-base) hover:text-fg hover:shadow-[inset_0_0_0_1px_var(--tn-accent)]"
          >
            {t}
          </a>
        </li>
      ))}
    </ul>
  );
}

/** The tag filter above a list: All posts, then every tag in use. */
export function TagFilter({ tags, current }: { tags: ReadonlyArray<{ tag: string; count: number }>; current?: string }) {
  const item = (href: string, label: string, count: number | null, on: boolean) => (
    <li key={href}>
      <a
        href={href}
        {...(on ? { "aria-current": "page" as const } : {})}
        className="inline-flex h-9 items-center gap-2 rounded-full bg-(--site-glass) px-3.5 text-copy-sm text-fg-muted shadow-[inset_0_0_0_1px_var(--site-line)] transition-[color,background-color,box-shadow] duration-(--dur-base) hover:bg-(--site-glass-hover) hover:text-fg aria-[current=page]:text-fg aria-[current=page]:shadow-[inset_0_0_0_1px_var(--tn-accent),0_0_16px_-4px_var(--site-glow-a)]"
      >
        {label}
        {count !== null && <span className="font-mono text-caption text-fg-muted">{count}</span>}
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
    <li className="reveal group relative grid gap-3 border-t border-line py-9 md:grid-cols-12 md:gap-6">
      <div className="font-mono text-caption text-fg-muted md:col-span-3 md:pt-2">
        <PostDate date={post.date} />
      </div>
      <div className="flex flex-col gap-3 md:col-span-9">
        <H className="text-display-md text-balance text-fg transition-colors duration-(--dur-base) group-hover:text-accent">
          <a
            href={post.path}
            className="after:absolute after:inset-0 focus-visible:outline-none"
            style={{ viewTransitionName: titleTransition(post.slug) }}
          >
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

/** A smaller card (related posts): a lit surface that lifts on hover, the
 * whole card one link. */
export function PostCard({ post }: { post: Post }) {
  return (
    <li className="reveal flex">
      <a href={post.path} className="surface surface-hover group flex w-full flex-col gap-3 p-6">
        <span className="font-mono text-[0.6875rem] uppercase tracking-[0.1em] text-fg-muted">{post.tags.join(" · ")}</span>
        <span className="text-title text-balance text-fg group-hover:text-accent" style={{ viewTransitionName: titleTransition(post.slug) }}>
          {post.title}
        </span>
        <span className="line-clamp-3 text-copy-sm text-pretty text-fg-muted">{post.description}</span>
        <span className="mt-auto flex gap-2 pt-2 text-caption text-fg-muted">
          <PostDate date={post.date} />
          <span aria-hidden="true">·</span>
          {post.readingMinutes} min read
        </span>
      </a>
    </li>
  );
}

/** A post's cover in the site's bezel (D-1834): a fixed-size <picture>
 * (AVIF, WebP, then the file; 2x where it exists, D-1838), loaded first. A
 * capture of the app names the build it shows, as every capture does. */
export function CoverFigure({
  cover,
  eager = true,
  caption = "full",
  sizes = "(min-width: 1024px) 896px, 100vw",
  className,
}: {
  cover: Cover;
  eager?: boolean;
  /** "build" keeps only the build label, which every capture carries. */
  caption?: "full" | "build";
  sizes?: string;
  className?: string;
}) {
  const base = cover.src.replace(/\.(png|jpe?g)$/, "");
  const modern = base !== cover.src;
  // Phones take the 800 px copy (D-1864), sharp screens the 2x one (D-1838).
  const widths = [
    ...(cover.narrow ? [`${base}-${COVER_NARROW}.{f} ${COVER_NARROW}w`] : []),
    `${base}.{f} ${cover.width}w`,
    ...(cover.retina ? [`${base}@2x.{f} ${cover.width * 2}w`] : []),
  ];
  const multi = widths.length > 1;
  const set = (f: string) => (multi ? widths.map((w) => w.replace("{f}", f)).join(", ") : `${base}.${f}`);
  return (
    <figure data-build={cover.build} className={cx("flex flex-col gap-4", className)}>
      <div className="frame">
        <div data-stage="" className="frame-screen">
          <picture>
            {modern && <source type="image/avif" srcSet={set("avif")} sizes={multi ? sizes : undefined} />}
            {modern && <source type="image/webp" srcSet={set("webp")} sizes={multi ? sizes : undefined} />}
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
      </div>
      {(cover.build || (caption === "full" && cover.caption)) && (
        <figcaption className="flex flex-col gap-1.5 text-caption text-pretty text-fg-muted sm:flex-row sm:items-baseline sm:gap-4">
          {cover.build && (
            <span className="shrink-0 font-mono text-[0.6875rem] uppercase tracking-[0.1em] text-fg-muted">
              {SITE.product} {cover.build} · Linux
            </span>
          )}
          {caption === "full" && cover.caption && <span>{cover.caption}</span>}
        </figcaption>
      )}
    </figure>
  );
}

/** How to follow the blog: the Atom feed, or an email. No newsletter form:
 * it would hand visitors' addresses to a third party (D-1853). */
export function Subscribe({ id = "subscribe", className }: { id?: string; className?: string }) {
  return (
    <aside aria-labelledby={`${id}-title`} className={cx("surface relative isolate overflow-hidden p-8 md:p-12", className)}>
      <div aria-hidden="true" className="bg-lens absolute inset-0 -z-10 opacity-80" />
      <Orbit
        id={id}
        className="absolute right-0 top-0 -z-10 h-[30rem] w-[48rem] max-w-none translate-x-1/3 -translate-y-1/3 opacity-70 [mask-image:radial-gradient(closest-side,black,transparent)]"
      />
      <div className="flex flex-col gap-8 md:flex-row md:items-end md:justify-between">
        <div className="flex max-w-measure flex-col gap-4">
          <p className="eyebrow">Subscribe</p>
          <h2 id={`${id}-title`} className="text-sheen text-display-md text-fg">
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
          <a href={FEED_PATH} className={siteButton("primary", "md")}>
            <Icon icon={Rss} size={16} />
            Atom feed
          </a>
          <a href={`mailto:${SITE.contactEmail}?subject=${encodeURIComponent(`${SITE.product} blog`)}`} className={siteButton("secondary", "md")}>
            <Icon icon={Mail} size={16} />
            Email us
          </a>
        </div>
      </div>
    </aside>
  );
}
