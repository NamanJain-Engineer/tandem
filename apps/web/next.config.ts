import type { NextConfig } from "next";

// `SITE_EXPORT=1` (`pnpm --filter web build:site`) builds only the public
// pages (`*.site.tsx`) as static files for Vercel. Other routes (the health
// handler) stay out of that export.
const siteExport = process.env.SITE_EXPORT === "1";

const nextConfig: NextConfig = siteExport
  ? {
      reactStrictMode: true,
      transpilePackages: ["@tandem/ui"],
      poweredByHeader: false,
      output: "export",
      pageExtensions: ["site.tsx", "site.ts"],
      trailingSlash: true,
      images: { unoptimized: true },
    }
  : {
      reactStrictMode: true,
      transpilePackages: ["@tandem/ui"],
      poweredByHeader: false,
      pageExtensions: ["site.tsx", "site.ts", "tsx", "ts"],
    };

export default nextConfig;
