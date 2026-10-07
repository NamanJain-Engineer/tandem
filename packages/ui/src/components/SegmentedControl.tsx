import { RadioGroup as R } from "radix-ui";
import type { ReactNode } from "react";
import { cx } from "../cx";
import { Tooltip } from "./Tooltip";

export type Segment = { id: string; label: string; icon?: ReactNode; kbd?: string[]; tooltip?: string };

export type SegmentedControlProps = {
  items: Segment[];
  value: string;
  onValueChange?: (id: string) => void;
  /** Accessible group name, e.g. "Work mode" (UI_SPEC §7.2). */
  label: string;
  size?: "sm" | "md";
  /** The title-bar mode switcher: 1px accent underline under the selection. */
  modeSwitch?: boolean;
  className?: string;
};

/** UI_SPEC §2.7: a radiogroup ("Work mode, Code, selected, 2 of 3"). */
export function SegmentedControl({ items, value, onValueChange, label, size = "md", modeSwitch, className }: SegmentedControlProps) {
  return (
    <R.Root
      aria-label={label}
      value={value}
      onValueChange={onValueChange}
      orientation="horizontal"
      className={cx("inline-flex items-center rounded-sm border border-border bg-surface-1 p-0.5", className)}
    >
      {items.map((item) => {
        const segment = (
          <R.Item
            key={item.id}
            value={item.id}
            className={cx(
              "group relative inline-flex items-center gap-1.5 rounded-xs px-3 font-medium text-fg-muted transition-colors duration-[var(--dur-fast)]",
              "hover:text-fg focus-visible:shadow-focus focus-visible:outline-none",
              "data-[state=checked]:bg-surface-3 data-[state=checked]:text-fg data-[state=checked]:shadow-1",
              size === "sm" ? "h-6 text-sm" : "h-7 text-base",
            )}
          >
            {item.icon && <span aria-hidden="true">{item.icon}</span>}
            {item.label}
            {modeSwitch && (
              <span
                aria-hidden="true"
                className="absolute inset-x-3 bottom-0.5 hidden h-px bg-accent group-data-[state=checked]:block"
              />
            )}
          </R.Item>
        );
        return item.tooltip ? (
          <Tooltip key={item.id} label={item.tooltip} kbd={item.kbd}>
            {segment}
          </Tooltip>
        ) : (
          segment
        );
      })}
    </R.Root>
  );
}
