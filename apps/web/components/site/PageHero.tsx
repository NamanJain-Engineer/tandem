import type { ReactNode } from "react";
import { cx } from "../../lib/cx";

/** The top of an inner page: eyebrow, the page's h1, a lead, and anything
 * else (actions, a meta line), over the site's grid and glow. */
export function PageHero({
  eyebrow,
  title,
  lead,
  children,
  className,
}: {
  eyebrow?: ReactNode;
  title: ReactNode;
  lead?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <section aria-label="Introduction" className={cx("relative isolate overflow-hidden bg-page", className)}>
      <div aria-hidden="true" className="bg-site-grid absolute inset-0 -z-10 [mask-image:linear-gradient(to_bottom,black_30%,transparent)]" />
      <div className="container-site flex flex-col items-start gap-5 pb-12 pt-16 lg:pb-16 lg:pt-20">
        {eyebrow && <div className="eyebrow">{eyebrow}</div>}
        <h1 className="max-w-[20em] text-display-xl text-balance text-fg">{title}</h1>
        {lead && <div className="max-w-measure text-lead text-pretty text-fg-muted">{lead}</div>}
        {children}
      </div>
    </section>
  );
}
