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
  /** left: title over lead; center: a centred stack; split: title left, lead right. */
  align?: "left" | "center" | "split";
  width?: "site" | "narrow";
  /** A lit hairline across the top of the section. */
  divider?: boolean;
  className?: string;
  children?: ReactNode;
};

/** A home or page section (D-1837): its header block, then the content. The
 * h2 is the section's accessible name. Never use it for #try (its id must
 * come first and nothing may nest inside it). */
export function Section({
  id,
  eyebrow,
  title,
  lead,
  aside,
  tone = "page",
  align = "left",
  width = "site",
  divider = false,
  className,
  children,
}: SectionProps) {
  return (
    <section
      id={id}
      aria-labelledby={`${id}-title`}
      className={cx(
        "relative scroll-mt-(--site-header-h) py-(--site-section-y)",
        tone === "raised" ? "bg-raised" : "bg-page",
        className,
      )}
    >
      {divider && <div aria-hidden="true" className="horizon absolute inset-x-0 top-0" />}
      <div className={cx("container-site flex flex-col gap-14 lg:gap-16", width === "narrow" && "max-w-narrow")}>
        <SectionHeader id={id} eyebrow={eyebrow} title={title} lead={lead} aside={aside} align={align} />
        {children}
      </div>
    </section>
  );
}

/** The eyebrow, h2, lead and aside of a section, in one of three shapes. */
export function SectionHeader({
  id,
  eyebrow,
  title,
  lead,
  aside,
  align = "left",
}: Pick<SectionProps, "id" | "eyebrow" | "title" | "lead" | "aside" | "align">) {
  const h2 = (
    <h2 id={`${id}-title`} className="text-sheen max-w-[20em] text-display-lg text-fg">
      {title}
    </h2>
  );
  if (align === "split")
    return (
      <header className="reveal grid gap-6 lg:grid-cols-12 lg:items-end lg:gap-6">
        <div className="flex flex-col gap-5 lg:col-span-6">
          {eyebrow && <p className="eyebrow">{eyebrow}</p>}
          {h2}
        </div>
        <div className="flex flex-col gap-5 lg:col-span-5 lg:col-start-8">
          {lead && <p className="text-lead text-pretty text-fg-muted">{lead}</p>}
          {aside && <div className="flex flex-wrap gap-x-6 gap-y-2">{aside}</div>}
        </div>
      </header>
    );
  return (
    <header
      className={cx(
        "reveal flex flex-col gap-5",
        align === "center" ? "items-center text-center" : "lg:flex-row lg:items-end lg:justify-between lg:gap-12",
      )}
    >
      <div className={cx("flex flex-col gap-5", align === "center" && "items-center")}>
        {eyebrow && <p className="eyebrow">{eyebrow}</p>}
        {h2}
        {lead && <p className="max-w-measure text-lead text-pretty text-fg-muted">{lead}</p>}
      </div>
      {aside && <div className="flex shrink-0 flex-wrap gap-x-6 gap-y-2">{aside}</div>}
    </header>
  );
}
