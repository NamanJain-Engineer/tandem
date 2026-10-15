import type { ReactNode } from "react";
import { cx } from "../../lib/cx";
import { Orbit } from "./Orbit";

/** The top of an inner page (D-1837): eyebrow, the page's h1, a lead, and
 * anything else (actions, a meta line), under the rings' light. `center`
 * stacks it in the middle (Download, Pricing). */
export function PageHero({
  eyebrow,
  title,
  lead,
  children,
  className,
  center = false,
}: {
  eyebrow?: ReactNode;
  title: ReactNode;
  lead?: ReactNode;
  children?: ReactNode;
  className?: string;
  center?: boolean;
}) {
  return (
    <section aria-label="Introduction" className={cx("grain relative isolate overflow-hidden bg-page", className)}>
      <div aria-hidden="true" className="bg-lens absolute inset-0 -z-10" />
      <Orbit
        id="page"
        className="absolute left-1/2 top-0 -z-10 h-[46rem] w-[90rem] max-w-none -translate-x-1/2 -translate-y-[16rem] [mask-image:linear-gradient(to_bottom,black_30%,transparent_85%)]"
      />
      <div
        className={cx(
          "container-site flex flex-col gap-6 pb-14 pt-20 lg:pb-20 lg:pt-28",
          center ? "items-center text-center" : "items-start",
        )}
      >
        {eyebrow && <div className="eyebrow">{eyebrow}</div>}
        <h1 className="text-sheen max-w-[18em] text-display-xl text-fg">{title}</h1>
        {lead && <div className="max-w-measure text-lead text-pretty text-fg-muted">{lead}</div>}
        {children}
      </div>
    </section>
  );
}
