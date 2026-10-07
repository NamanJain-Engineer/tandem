import { securityTxt } from "../../../lib/site";

export const dynamic = "force-static";

/** RFC 9116. */
export function GET(): Response {
  return new Response(securityTxt(), { headers: { "content-type": "text/plain; charset=utf-8" } });
}
