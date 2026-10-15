import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { TagIndex } from "../../../../components/blog/TagIndex";
import { POSTS, tagPath, tagsOf } from "../../../../lib/blog/posts";
import { SITE } from "../../../../lib/site";
import { FEED_ALTERNATE, blogOpenGraph } from "../../page.site";

type Params = { params: Promise<{ tag: string }> };

/** One page per tag in use (F-WB-06 AC4). */
export const dynamicParams = false;

export function generateStaticParams(): Array<{ tag: string }> {
  return tagsOf(POSTS).map((t) => ({ tag: t.tag }));
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { tag } = await params;
  const description = `${SITE.product} blog posts tagged ${tag}.`;
  return {
    title: `Posts tagged ${tag}`,
    description,
    alternates: { canonical: tagPath(tag), types: FEED_ALTERNATE },
    openGraph: blogOpenGraph(`Posts tagged ${tag}`, description, tagPath(tag)),
  };
}

export default async function TagPage({ params }: Params) {
  const { tag } = await params;
  if (!POSTS.some((p) => p.tags.includes(tag))) notFound();
  return <TagIndex tag={tag} posts={POSTS} />;
}
