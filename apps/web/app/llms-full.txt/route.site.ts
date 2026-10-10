import { llmsFullTxt } from "../../lib/discovery";

export const dynamic = "force-static";

export function GET(): Response {
  return new Response(llmsFullTxt(), { headers: { "content-type": "text/plain; charset=utf-8" } });
}
