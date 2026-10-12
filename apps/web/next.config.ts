import type { NextConfig } from "next";
import { createMDX } from "fumadocs-mdx/next";

const api = process.env.API_INTERNAL_URL ?? "http://127.0.0.1:8787";

// `SITE_EXPORT=1` (`pnpm --filter web build:site`) builds only the public
// pages (`*.site.tsx`) as static files for Vercel. The account
// portal and route handlers need the API and stay out of that export.
const siteExport = process.env.SITE_EXPORT === "1";

const nextConfig: NextConfig = siteExport
  ? {
      reactStrictMode: true,
      transpilePackages: ["@tandem/ui", "@tandem/protocol", "@tandem/remote"],
      poweredByHeader: false,
      output: "export",
      pageExtensions: ["site.tsx", "site.ts"],
      trailingSlash: true,
      // Next's own trailing-slash redirects treat a last segment like
      // "0.6.0" as a file and strip its slash, so /changelog/0.6.0/ answered
      // 308 on Vercel (which copies them from routes-manifest.json). The
      // "add a slash" half lives in vercel.json instead (D-1815), and
      // scripts/check-routes.mjs fails the build if any page's URL redirects.
      skipTrailingSlashRedirect: true,
      images: { unoptimized: true },
    }
  : {
      reactStrictMode: true,
      transpilePackages: ["@tandem/ui", "@tandem/protocol", "@tandem/remote"],
      poweredByHeader: false,
      pageExtensions: ["site.tsx", "site.ts", "tsx", "ts"],
      // The API is served first-party under /v1 so auth cookies belong to this origin.
      async rewrites() {
        return [{ source: "/v1/:path*", destination: `${api}/v1/:path*` }];
      },
    };

// /docs pages are MDX (F-WB-02, Fumadocs).
export default createMDX()(nextConfig);
