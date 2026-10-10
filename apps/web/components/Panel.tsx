import type { ReactNode } from "react";

export function Card({ children }: { children: ReactNode }) {
  return <section className="rounded-md border border-border bg-surface-2 p-6">{children}</section>;
}

export function Notice({
  tone = "info",
  children,
}: {
  tone?: "info" | "error" | "success";
  children: ReactNode;
}) {
  const color = tone === "error" ? "text-danger" : tone === "success" ? "text-done-text" : "text-fg-muted";
  return (
    <p role={tone === "error" ? "alert" : "status"} className={`text-sm ${color}`}>
      {children}
    </p>
  );
}
