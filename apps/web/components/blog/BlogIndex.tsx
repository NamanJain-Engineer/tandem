import { Mark } from "../site/Mark";
import { Orbit } from "../site/Orbit";
import { PageHero } from "../site/PageHero";
import { SiteFrame } from "../site/SiteFrame";
import { TextLink } from "../site/TextLink";
import { BLOG_PATH, FEED_PATH, tagsOf, titleTransition, type Post } from "../../lib/blog/posts";
import { CoverFigure, PostMeta, PostRow, Subscribe, TagFilter, TagList } from "./parts";

/** The lead story: the newest post, large. Its cover in the bezel if it has
 * one, else a typographic panel on the stage under the rings' light, so a
 * post without images still leads well. */
function Featured({ post }: { post: Post }) {
  return (
    <article data-featured="" className="group relative grid gap-10 lg:grid-cols-12 lg:items-center lg:gap-14">
      <div className="rise transition-transform duration-(--dur-slow) ease-(--ease-out-site) motion-safe:group-hover:-translate-y-1 lg:col-span-7">
        {post.cover ? (
          <CoverFigure cover={post.cover} caption="build" sizes="(min-width: 1024px) 680px, 100vw" />
        ) : (
          <div className="frame">
            <div
              data-stage=""
              aria-hidden="true"
              className="frame-screen relative isolate flex aspect-[16/10] flex-col justify-between p-8 md:p-10"
            >
              <Orbit id="featured" className="absolute inset-0 -z-10 h-full w-full opacity-90" />
              <span className="flex items-center gap-2 font-mono text-eyebrow uppercase tracking-[0.1em] text-stage-muted">
                <Mark />
                {post.tags.join(" · ")}
              </span>
              <span className="max-w-[16em] text-display-lg text-balance text-stage-fg">{post.title}</span>
            </div>
          </div>
        )}
      </div>
      <div className="flex flex-col items-start gap-5 lg:col-span-5">
        <p className="eyebrow">Latest</p>
        <h2 className="text-display-lg text-balance text-fg transition-colors duration-(--dur-base) group-hover:text-accent">
          <a
            href={post.path}
            className="after:absolute after:inset-0 focus-visible:outline-none"
            style={{ viewTransitionName: titleTransition(post.slug) }}
          >
            {post.title}
          </a>
        </h2>
        <p className="text-lead text-pretty text-fg-muted">{post.description}</p>
        <PostMeta post={post} />
        <TagList tags={post.tags} />
      </div>
    </article>
  );
}

function Empty() {
  return (
    <div data-empty="" className="surface flex flex-col items-start gap-4 p-8 md:p-12">
      <h2 className="text-display-md text-fg">No posts yet</h2>
      <p className="max-w-measure text-copy text-pretty text-fg-muted">
        The first post is on its way. Add the Atom feed to your reader and it will show up there. Until then, the
        changelog has the notes for every release.
      </p>
      <div className="flex flex-wrap gap-x-6">
        <TextLink href={FEED_PATH}>Atom feed</TextLink>
        <TextLink href="/changelog/">Changelog</TextLink>
      </div>
    </div>
  );
}

/** /blog/ (F-WB-06 AC2): the newest post featured, then the rest, newest
 * first, then the ways to follow along. */
export function BlogIndex({ posts }: { posts: readonly Post[] }) {
  const [first, ...rest] = posts;
  return (
    <SiteFrame current={BLOG_PATH}>
      <PageHero
        eyebrow="Blog"
        title="Notes from building Vibezeo"
        lead="What ships in each release, how the app works under the hood, and how to get more from the coding agents you already use."
      />
      <div className="container-site flex flex-col gap-24 pb-(--site-section-y) lg:gap-28">
        {first ? <Featured post={first} /> : <Empty />}
        {first && (
          <section aria-labelledby="all-posts" className="flex flex-col gap-8">
            <div className="reveal flex flex-col gap-6 md:flex-row md:items-end md:justify-between">
              <h2 id="all-posts" className="text-sheen text-display-md text-fg">
                All posts
              </h2>
              <TagFilter tags={tagsOf(posts)} />
            </div>
            {rest.length > 0 ? (
              <ol className="flex flex-col border-b border-line">
                {rest.map((p) => (
                  <PostRow key={p.slug} post={p} />
                ))}
              </ol>
            ) : (
              <p className="border-t border-line pt-8 text-copy text-fg-muted">That&apos;s every post so far.</p>
            )}
          </section>
        )}
        <Subscribe className="reveal" />
      </div>
    </SiteFrame>
  );
}
