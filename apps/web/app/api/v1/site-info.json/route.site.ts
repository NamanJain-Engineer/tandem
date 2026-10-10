import { siteInfo } from "../../../../lib/discovery";

export const dynamic = "force-static";

export function GET(): Response {
  return new Response(JSON.stringify(siteInfo(), null, 2), {
    headers: { "content-type": "application/json" },
  });
}
