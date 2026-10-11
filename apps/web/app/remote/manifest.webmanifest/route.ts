import { SITE } from "../../../lib/site";

/** The phone companion's web app manifest, so it installs to the home screen (F-RM-03). */
export function GET() {
  return Response.json(
    {
      name: `${SITE.product} Remote`,
      short_name: SITE.product,
      start_url: "/remote",
      scope: "/remote",
      display: "standalone",
      background_color: "#060a12",
      theme_color: "#060a12",
      icons: [
        { src: "/icon.png", sizes: "512x512", type: "image/png" },
        { src: "/favicon.svg", sizes: "any", type: "image/svg+xml" },
      ],
    },
    { headers: { "content-type": "application/manifest+json" } },
  );
}
