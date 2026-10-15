import type { ReactNode } from "react";
import { Mark } from "./Mark";
import { Orbit } from "./Orbit";

/** A closing call to action (D-1837): the mark at the centre of the rings,
 * a large heading, a line and the actions. Used at the foot of the home page
 * and the inner pages. */
export function CtaBand({
  id,
  title,
  lead,
  children,
}: {
  id: string;
  title: ReactNode;
  lead?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section aria-labelledby={`${id}-title`} className="grain relative isolate overflow-hidden bg-page">
      <div aria-hidden="true" className="horizon absolute inset-x-0 top-0" />
      <div aria-hidden="true" className="absolute inset-0 -z-10 bg-[radial-gradient(50%_60%_at_50%_100%,var(--site-glow-a),transparent_75%)]" />
      <Orbit
        id={id}
        turn
        className="absolute left-1/2 top-1/2 -z-10 h-[52rem] w-[82rem] max-w-none -translate-x-1/2 -translate-y-[38%] opacity-90 [mask-image:radial-gradient(closest-side,black_45%,transparent)]"
      />
      <div className="container-site flex flex-col items-center gap-7 py-[calc(var(--site-section-y)*1.15)] text-center">
        <span className="reveal inline-flex size-16 items-center justify-center rounded-2xl surface">
          <Mark size={34} />
        </span>
        <h2 id={`${id}-title`} className="reveal text-sheen max-w-[16em] text-display-2xl text-fg">
          {title}
        </h2>
        {lead && <p className="reveal max-w-measure text-lead text-pretty text-fg-muted">{lead}</p>}
        <div className="reveal flex flex-wrap justify-center gap-3 pt-1">{children}</div>
      </div>
    </section>
  );
}
