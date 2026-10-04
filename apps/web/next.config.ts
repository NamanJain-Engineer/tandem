import type { NextConfig } from "next";

const api = process.env.API_INTERNAL_URL ?? "http://127.0.0.1:8787";

const nextConfig: NextConfig = {
  reactStrictMode: true,
  transpilePackages: ["@tandem/ui", "@tandem/protocol"],
  poweredByHeader: false,
  // The API is served first-party under /v1 so auth cookies belong to this origin.
  async rewrites() {
    return [{ source: "/v1/:path*", destination: `${api}/v1/:path*` }];
  },
};

export default nextConfig;
