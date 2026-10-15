import type { ReactNode } from "react";
import { cx } from "../../lib/cx";

/** A card (D-1830): a lit fill and a gradient edge. `href` makes the whole
 * card a link that lifts on hover; `as` keeps lists semantic. */
export function Card({
  as: Tag = "div",
  href,
  tone = "page",
  className,
  children,
  id,
}: {
  as?: "div" | "li" | "article" | "figure";
  href?: string;
  tone?: "page" | "raised";
  className?: string;
  children: ReactNode;
  id?: string;
}) {
  const surface = cx(tone === "raised" ? "surface-raised" : "surface", href && "surface-hover", "flex flex-col");
  if (href)
    return (
      <Tag id={id} className={cx("flex", className)}>
        <a href={href} className={cx(surface, "group w-full gap-4 p-6 lg:p-7")}>
          {children}
        </a>
      </Tag>
    );
  return (
    <Tag id={id} className={cx(surface, "gap-4 p-6 lg:p-7", className)}>
      {children}
    </Tag>
  );
}

/** A small mono label: a version, a platform, a step number. */
export function Tag({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span
      className={cx(
        "inline-flex h-6 items-center gap-1.5 rounded-full px-2.5 font-mono text-[0.6875rem] uppercase tracking-[0.1em] text-fg-muted shadow-[inset_0_0_0_1px_var(--site-line-strong)]",
        className,
      )}
    >
      {children}
    </span>
  );
}
