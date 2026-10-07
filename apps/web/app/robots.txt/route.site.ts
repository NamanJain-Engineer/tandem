import { robotsTxt } from "../../lib/site";

export const dynamic = "force-static";

export function GET(): Response {
  return new Response(robotsTxt(), { headers: { "content-type": "text/plain; charset=utf-8" } });
}
