import { ArrowLeft, ArrowRight } from "lucide-react";
import type { ReactNode } from "react";
import { Icon } from "../site/Icon";
import { SiteFrame } from "../site/SiteFrame";
import { TextLink } from "../site/TextLink";
import { BLOG_PATH, type Post } from "../../lib/blog/posts";
import { cx } from "../../lib/cx";
import { CoverFigure, PostDate, PostMeta, Subscribe, TagList } from "./parts";

export type TocItem = { title: ReactNode; url: string; depth: number };

/** The reading column and the rails either side of it (lg and up). */
const COLUMNS = "lg:grid lg:grid-cols-[minmax(0,1fr)_minmax(0,42rem)_minmax(0,1fr)] lg:gap-x-12";

/** "On this page": the post's h2s and h3s. A plain list, sticky beside the
 * text; without a script it can't follow the scroll, and doesn't try. */
function Contents({ toc }: { toc: readonly TocItem[] }) {
  return (
    <nav aria-labelledby="contents-title" className="hidden lg:sticky lg:top-24 lg:col-start-1 lg:row-start-1 lg:block lg:self-start lg:justify-self-end">
      <p id="contents-title" className="eyebrow mb-4">
        On this page
      </p>
      <ol className="flex max-w-56 flex-col gap-2 border-l border-border text-copy-sm">
        {toc.map((t) => (
          <li key={t.url} className={cx("-ml-px border-l border-transparent", t.depth > 2 ? "pl-7" : "pl-4")}>
            <a href={t.url} className="block text-pretty text-fg-muted transition-colors duration-(--dur-instant) hover:text-fg">
              {t.title}
            </a>
          </li>
        ))}
      </ol>
    </nav>
  );
}

function Neighbour({ post, rel }: { post: Post; rel: "newer" | "older" }) {
  return (
    <a
      href={post.path}
      className={cx(
        "group flex flex-col gap-2 rounded-lg border border-border bg-card p-6 transition-colors duration-(--dur-instant) hover:border-border-strong",
        rel === "older" && "sm:items-end sm:text-right",
      )}
    >
      <span className="inline-flex items-center gap-1.5 text-caption text-fg-muted">
        {rel === "newer" && <Icon icon={ArrowLeft} size={14} />}
        {rel === "newer" ? "Newer post" : "Older post"}
        {rel === "older" && <Icon icon={ArrowRight} size={14} />}
      </span>
      <span className="text-title text-balance text-fg group-hover:text-accent">{post.title}</span>
      <PostDate date={post.date} className="text-caption text-fg-muted" />
    </a>
  );
}

/** /blog/<slug>/ (F-WB-06 AC3): the title block, the optional cover, the
 * body in a 68-character column with its contents beside it, then the
 * neighbouring posts and the ways to follow along. */
export function PostLayout({
  post,
  newer,
  older,
  toc,
  children,
}: {
  post: Post;
  newer?: Post;
  older?: Post;
  toc: readonly TocItem[];
  children: ReactNode;
}) {
  return (
    <SiteFrame current={BLOG_PATH}>
      <article className="relative isolate">
        <div aria-hidden="true" className="bg-site-grid absolute inset-x-0 top-0 -z-10 h-[28rem] [mask-image:linear-gradient(to_bottom,black_20%,transparent)]" />
        <header className={cx("container-site pb-10 pt-12 lg:pb-14 lg:pt-16", COLUMNS)}>
          <div className="flex flex-col items-start gap-6 lg:col-start-2">
            <a href={BLOG_PATH} className="inline-flex min-h-8 items-center gap-1.5 text-copy-sm text-fg-muted hover:text-fg">
              <Icon icon={ArrowLeft} size={14} />
              Blog
            </a>
            <TagList tags={post.tags} />
            <h1 className="text-display-xl text-balance text-fg">{post.title}</h1>
            <p className="text-lead text-pretty text-fg-muted">{post.description}</p>
            <PostMeta post={post} className="border-t border-border pt-6 w-full" />
          </div>
        </header>

        {post.cover && (
          <div className="container-site pb-12 lg:pb-16">
            <CoverFigure cover={post.cover} className="mx-auto max-w-narrow" />
          </div>
        )}

        <div className={cx("container-site", COLUMNS)}>
          {toc.length > 1 && <Contents toc={toc} />}
          <div data-post-body="" className="min-w-0 lg:col-start-2 lg:row-start-1">
            {children}
          </div>
        </div>

        <footer className={cx("container-site pb-16 pt-16", COLUMNS)}>
          <div className="flex flex-col gap-10 lg:col-start-2">
            <div className="flex flex-wrap items-center justify-between gap-4 border-t border-border pt-8">
              <TagList tags={post.tags} />
              <TextLink href={BLOG_PATH}>All posts</TextLink>
            </div>
            {(newer || older) && (
              <nav aria-label="More posts" className="grid gap-4 sm:grid-cols-2">
                {newer ? <Neighbour post={newer} rel="newer" /> : <span className="hidden sm:block" />}
                {older && <Neighbour post={older} rel="older" />}
              </nav>
            )}
          </div>
        </footer>
      </article>
      <div className="container-site pb-(--site-section-y)">
        <Subscribe />
      </div>
    </SiteFrame>
  );
}
