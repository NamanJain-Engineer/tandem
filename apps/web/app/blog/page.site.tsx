import type { Metadata } from "next";
import { BlogIndex } from "../../components/blog/BlogIndex";
import { BLOG_PATH, FEED_PATH, POSTS } from "../../lib/blog/posts";
import { SITE } from "../../lib/site";

/** The feed, in every blog page's <head> (F-WB-06 AC5). */
export const FEED_ALTERNATE = { "application/atom+xml": [{ url: FEED_PATH, title: `${SITE.product} blog` }] };

/** Share cards for the index and tag pages: the site's image, the page's URL. */
export const blogOpenGraph = (title: string, description: string, url: string): Metadata["openGraph"] => ({
  type: "website",
  title,
  description,
  url,
  siteName: SITE.product,
  images: [{ url: "/og.png", width: 1200, height: 630, alt: `${SITE.product}: ${SITE.tagline}` }],
});

const description = `Notes from building ${SITE.product}: what ships in each release, and how it works.`;

export const metadata: Metadata = {
  title: "Blog",
  description,
  alternates: { canonical: BLOG_PATH, types: FEED_ALTERNATE },
  openGraph: blogOpenGraph(`${SITE.product} blog`, description, BLOG_PATH),
};

/** /blog/ (F-WB-06). */
export default function BlogPage() {
  return <BlogIndex posts={POSTS} />;
}
