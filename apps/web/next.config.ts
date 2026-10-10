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
      transpilePackages: ["@tandem/ui", "@tandem/protocol"],
      poweredByHeader: false,
      output: "export",
      pageExtensions: ["site.tsx", "site.ts"],
      trailingSlash: true,
      images: { unoptimized: true },
    }
  : {
      reactStrictMode: true,
      transpilePackages: ["@tandem/ui", "@tandem/protocol"],
      poweredByHeader: false,
      pageExtensions: ["site.tsx", "site.ts", "tsx", "ts"],
      // The API is served first-party under /v1 so auth cookies belong to this origin.
      async rewrites() {
        return [{ source: "/v1/:path*", destination: `${api}/v1/:path*` }];
      },
    };

// /docs pages are MDX (F-WB-02, Fumadocs).
export default createMDX()(nextConfig);
