import { Tooltip as T } from "radix-ui";
import type { ReactElement, ReactNode } from "react";
import { Kbd } from "./Kbd";

export type TooltipProps = {
  label: ReactNode;
  /** Shortcut shown right-aligned in the tooltip. */
  kbd?: string[];
  side?: "top" | "right" | "bottom" | "left";
  /** Must be a focusable element (or a span wrapping a disabled one). */
  children: ReactElement;
  /** Render nothing extra (e.g. on touch). */
  disabled?: boolean;
  /** Controlled open state (gallery, tests). */
  open?: boolean;
};

/** UI_SPEC §2.8: 500 ms delay, 0 ms if another tooltip was open in the last 800 ms. */
export function TooltipProvider({ children }: { children: ReactNode }) {
  return (
    <T.Provider delayDuration={500} skipDelayDuration={800}>
      {children}
    </T.Provider>
  );
}

export function Tooltip({ label, kbd, side = "bottom", children, disabled, open }: TooltipProps) {
  if (disabled) return children;
  return (
    <T.Root open={open}>
      <T.Trigger asChild>{children}</T.Trigger>
      <T.Portal>
        <T.Content
          side={side}
          sideOffset={6}
          className="tn-pop z-[var(--z-popover)] flex max-w-[280px] items-center gap-3 rounded-sm bg-surface-3 px-2 py-1.5 text-sm text-fg shadow-e2"
        >
          <span>{label}</span>
          {kbd && <Kbd keys={kbd} className="ml-auto" />}
        </T.Content>
      </T.Portal>
    </T.Root>
  );
}
