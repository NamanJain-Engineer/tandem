import { ArrowRight } from "lucide-react";
import type { ReactNode } from "react";
import { cx } from "../../lib/cx";
import { Icon } from "./Icon";

/** A standalone link with an arrow icon that nudges on hover ("Release notes"). */
export function TextLink({ href, children, className }: { href: string; children: ReactNode; className?: string }) {
  return (
    <a
      href={href}
      className={cx(
        "link-arrow inline-flex min-h-8 items-center gap-1.5 font-medium text-fg hover:text-fg-muted",
        className,
      )}
    >
      {children}
      <Icon icon={ArrowRight} size={16} />
    </a>
  );
}
