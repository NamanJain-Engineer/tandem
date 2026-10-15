import { POSTS, atomFeed } from "../../../lib/blog/posts";

export const dynamic = "force-static";

/** /blog/feed.xml: the blog's Atom feed (F-WB-06 AC5). */
export function GET(): Response {
  return new Response(atomFeed(POSTS), { headers: { "content-type": "application/atom+xml; charset=utf-8" } });
}
