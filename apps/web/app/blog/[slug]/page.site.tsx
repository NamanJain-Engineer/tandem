import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { blog } from "../../../.source";
import { PostLayout } from "../../../components/blog/PostLayout";
import { BLOG_MDX, PROSE } from "../../../components/blog/mdx";
import { OG_SIZE, POSTS, jsonLdText, neighbours, ogImagePath, related } from "../../../lib/blog/posts";
import { SITE } from "../../../lib/site";
import { FEED_ALTERNATE } from "../page.site";

type Params = { params: Promise<{ slug: string }> };

/** One page per post (F-WB-06 AC3). Drafts are in POSTS only under
 * `next dev`, so the export never has them. */
export const dynamicParams = false;

export function generateStaticParams(): Array<{ slug: string }> {
  return POSTS.map((p) => ({ slug: p.slug }));
}

const find = (slug: string) => POSTS.find((p) => p.slug === slug);

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const post = find((await params).slug);
  if (!post) notFound();
  // Drawn at build time from the title and description (D-1859).
  const image = { url: ogImagePath(post.slug), ...OG_SIZE, alt: `${post.title}, on the ${SITE.product} blog` };
  return {
    title: post.title,
    description: post.description,
    authors: [{ name: post.author }],
    alternates: { canonical: post.path, types: FEED_ALTERNATE },
    openGraph: {
      type: "article",
      title: post.title,
      description: post.description,
      url: post.path,
      siteName: SITE.product,
      authors: [post.author],
      tags: post.tags,
      images: [image],
    },
    twitter: { card: "summary_large_image", title: post.title, description: post.description, images: [image.url] },
  };
}

export default async function PostPage({ params }: Params) {
  const post = find((await params).slug);
  const entry = blog.find((e) => e.info.path === post?.file);
  if (!post || !entry) notFound();
  const Body = entry.body;
  const { newer, older } = neighbours(POSTS, post.slug);
  return (
    <>
      {/* Data for search engines, not code: the one script a blog page may carry (D-1852). */}
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: jsonLdText(post) }} />
      <PostLayout
        post={post}
        newer={newer}
        older={older}
        related={related(POSTS, post.slug, [newer, older].filter((p) => p !== undefined))}
        toc={entry.toc.filter((t) => t.depth <= 3)}
      >
        <div className={PROSE}>
          <Body components={BLOG_MDX} />
        </div>
      </PostLayout>
    </>
  );
}
