import { PageHero } from "../site/PageHero";
import { SiteFrame } from "../site/SiteFrame";
import { BLOG_PATH, tagsOf, type Post } from "../../lib/blog/posts";
import { PostRow, Subscribe, TagFilter } from "./parts";

/** /blog/tags/<tag>/ (F-WB-06 AC4): exactly the posts with that tag. */
export function TagIndex({ tag, posts }: { tag: string; posts: readonly Post[] }) {
  const tagged = posts.filter((p) => p.tags.includes(tag));
  return (
    <SiteFrame current={BLOG_PATH}>
      <PageHero
        eyebrow={
          <a href={BLOG_PATH} className="hover:text-fg">
            Blog
          </a>
        }
        title={
          <>
            Posts tagged <span className="font-mono">{tag}</span>
          </>
        }
        lead={`${tagged.length} ${tagged.length === 1 ? "post" : "posts"}, newest first.`}
      />
      <div className="container-site flex flex-col gap-16 pb-(--site-section-y)">
        <TagFilter tags={tagsOf(posts)} current={tag} />
        <ol className="flex flex-col border-b border-line">
          {tagged.map((p) => (
            <PostRow key={p.slug} post={p} headingLevel={2} />
          ))}
        </ol>
        <Subscribe className="reveal" />
      </div>
    </SiteFrame>
  );
}
