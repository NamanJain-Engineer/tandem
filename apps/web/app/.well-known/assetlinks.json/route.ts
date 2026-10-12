import { assetLinks } from "../../../lib/appLinks";

export function GET() {
  const body = assetLinks(process.env.ANDROID_CERT_SHA256);
  if (!body) return new Response("Not found", { status: 404 });
  return Response.json(body, { headers: { "cache-control": "public, max-age=3600" } });
}
