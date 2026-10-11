import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import { SITE } from "../../lib/site";

export const metadata: Metadata = {
  title: "Remote",
  description: `${SITE.product} on your phone: agent status, approvals and threads from your computer, end-to-end encrypted.`,
  manifest: "/remote/manifest.webmanifest",
  robots: { index: false, follow: false },
  appleWebApp: { capable: true, title: SITE.product, statusBarStyle: "black-translucent" },
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  themeColor: "#060a12",
};

export default function RemoteLayout({ children }: { children: ReactNode }) {
  return children;
}
