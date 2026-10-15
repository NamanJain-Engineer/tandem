import { cx } from "../cx";

export type SiteButtonVariant = "primary" | "secondary" | "ghost";
export type SiteButtonSize = "sm" | "md" | "lg";

/** The public site's buttons (D-1835), as classes for links: the primary is
 * the accent with a lit top edge and a soft glow, the secondary is glass with
 * a hairline. The look lives in site.css (`btn-*`); the focus ring comes from
 * the site's `:focus-visible` rule. */
export function siteButton(variant: SiteButtonVariant = "secondary", size: SiteButtonSize = "md", className?: string): string {
  return cx("btn", VARIANT[variant], SIZE[size], "focus-visible:outline-none", className);
}

// Whole class names, so Tailwind's scanner finds every one.
const VARIANT: Record<SiteButtonVariant, string> = {
  primary: "btn-primary",
  secondary: "btn-secondary",
  ghost: "btn-ghost",
};
const SIZE: Record<SiteButtonSize, string> = { sm: "btn-sm", md: "btn-md", lg: "btn-lg" };
