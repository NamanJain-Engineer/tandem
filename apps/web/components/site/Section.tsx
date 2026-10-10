import type { ReactNode } from "react";
import { cx } from "../../lib/cx";

export type SectionProps = {
  id: string;
  eyebrow?: string;
  title: ReactNode;
  lead?: ReactNode;
  /** A link or two beside the title (desktop) or under the lead (phone). */
  aside?: ReactNode;
  tone?: "page" | "raised";
  align?: "left" | "center";
  width?: "site" | "narrow";
  className?: string;
  children?: ReactNode;
};

/** A home or page section: its header block, then the content. The h2 is the
 * section's accessible name. Never use it for #try (its id must come first
 * and nothing may nest inside it). */
export function Section({
  id,
  eyebrow,
  title,
  lead,
  aside,
  tone = "page",
  align = "left",
  width = "site",
  className,
  children,
}: SectionProps) {
  return (
    <section
      id={id}
      aria-labelledby={`${id}-title`}
      className={cx(
        "scroll-mt-(--site-header-h) py-(--site-section-y)",
        tone === "raised" ? "bg-raised" : "bg-page",
        className,
      )}
    >
      <div className={cx("container-site flex flex-col gap-12", width === "narrow" && "max-w-narrow")}>
        <header
          className={cx(
            "reveal flex flex-col gap-4",
            align === "center" ? "items-center text-center" : "lg:flex-row lg:items-end lg:justify-between lg:gap-12",
          )}
        >
          <div className={cx("flex flex-col gap-4", align === "center" && "items-center")}>
            {eyebrow && <p className="eyebrow">{eyebrow}</p>}
            <h2 id={`${id}-title`} className="max-w-[22em] text-display-lg text-balance text-fg">
              {title}
            </h2>
            {lead && <p className="max-w-measure text-lead text-pretty text-fg-muted">{lead}</p>}
          </div>
          {aside && <div className="flex shrink-0 flex-wrap gap-x-6 gap-y-2">{aside}</div>}
        </header>
        {children}
      </div>
    </section>
  );
}
