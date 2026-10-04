import type { ReactNode } from "react";
import "./globals.css";

export const metadata = {
  title: "Tandem",
  description: "A desktop workspace for the coding agents you already use.",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" data-theme="dark">
      <body>{children}</body>
    </html>
  );
}
