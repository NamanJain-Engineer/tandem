import { appleAppSiteAssociation } from "../../../lib/appLinks";

export function GET() {
  const body = appleAppSiteAssociation(process.env.APPLE_TEAM_ID);
  if (!body) return new Response("Not found", { status: 404 });
  return Response.json(body, { headers: { "cache-control": "public, max-age=3600" } });
}
