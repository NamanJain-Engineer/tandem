import { openApi } from "../../lib/discovery";

export const dynamic = "force-static";

export function GET(): Response {
  return new Response(JSON.stringify(openApi(), null, 2), {
    headers: { "content-type": "application/json" },
  });
}
