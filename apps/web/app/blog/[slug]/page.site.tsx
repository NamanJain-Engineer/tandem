import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { blog } from "../../../.source";
import { PostLayout } from "../../../components/blog/PostLayout";
import { BLOG_MDX, PROSE } from "../../../components/blog/mdx";
import { POSTS, jsonLdText, neighbours } from "../../../lib/blog/posts";
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
  const image = post.cover
    ? { url: post.cover.src, width: post.cover.width, height: post.cover.height, alt: post.cover.alt }
    : { url: "/og.png", width: 1200, height: 630, alt: `${SITE.product}: ${SITE.tagline}` };
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
      <PostLayout post={post} newer={newer} older={older} toc={entry.toc.filter((t) => t.depth <= 3)}>
        <div className={PROSE}>
          <Body components={BLOG_MDX} />
        </div>
      </PostLayout>
    </>
  );
}
