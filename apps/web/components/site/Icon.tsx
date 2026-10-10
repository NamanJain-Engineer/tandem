import type { LucideIcon } from "lucide-react";

/** A lucide icon as the site draws it: 1.75 stroke, hidden from assistive
 * tech (always beside text), and no `xmlns`, which inline SVG in HTML doesn't
 * need and whose "2000" the no-year check would read as a year. Server-rendered:
 * no script. */
export function Icon({ icon: Glyph, size = 16, className }: { icon: LucideIcon; size?: number; className?: string }) {
  return <Glyph size={size} strokeWidth={1.75} aria-hidden="true" focusable="false" xmlns={undefined} className={className} />;
}
