import type { ReactNode } from "react";
import { Popover as P } from "radix-ui";
import { cx } from "../cx";

export type PopoverProps = {
  trigger: ReactNode;
  children: ReactNode;
  /** Accessible name of the panel. */
  label: string;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  align?: "start" | "center" | "end";
  side?: "top" | "right" | "bottom" | "left";
  className?: string;
};

/** An anchored panel (surface-3, shadow-2): the notifications popover
 * (UI-40) and other rich anchored content. Esc and an outside click close it. */
export function Popover({ trigger, children, label, open, onOpenChange, align = "end", side = "bottom", className }: PopoverProps) {
  return (
    <P.Root open={open} onOpenChange={onOpenChange}>
      <P.Trigger asChild>{trigger}</P.Trigger>
      <P.Portal>
        <P.Content
          aria-label={label}
          align={align}
          side={side}
          sideOffset={6}
          collisionPadding={8}
          className={cx(
            "tn-pop z-[var(--z-popover)] flex max-h-[min(560px,var(--radix-popover-content-available-height))] flex-col overflow-hidden rounded-md border border-border bg-surface-3 shadow-2 outline-none",
            className,
          )}
        >
          {children}
        </P.Content>
      </P.Portal>
    </P.Root>
  );
}
