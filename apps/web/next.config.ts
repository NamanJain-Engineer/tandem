import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  reactStrictMode: true,
  transpilePackages: ["@tandem/ui", "@tandem/protocol"],
  poweredByHeader: false,
};

export default nextConfig;
