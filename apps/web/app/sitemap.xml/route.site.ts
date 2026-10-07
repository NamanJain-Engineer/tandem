import { sitemapXml } from "../../lib/site";

export const dynamic = "force-static";

export function GET(): Response {
  return new Response(sitemapXml(), { headers: { "content-type": "application/xml; charset=utf-8" } });
}
