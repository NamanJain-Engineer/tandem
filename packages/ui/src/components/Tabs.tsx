import { Tabs as T } from "radix-ui";
import type { ReactNode } from "react";
import { cx } from "../cx";

export type TabItem = { id: string; label: ReactNode; icon?: ReactNode; count?: number; disabled?: boolean };

export type TabsProps = {
  items: TabItem[];
  value: string;
  onValueChange?: (id: string) => void;
  variant?: "underline" | "pill";
  /** Manual activation for heavy tabs (UI_SPEC §2.6). */
  activation?: "automatic" | "manual";
  label: string;
  /** Panel content for a tab id. */
  renderPanel: (id: string) => ReactNode;
  className?: string;
  overflow?: "scroll" | "menu";
};

export function Tabs({ items, value, onValueChange, variant = "underline", activation = "automatic", label, renderPanel, className }: TabsProps) {
  return (
    <T.Root value={value} onValueChange={onValueChange} activationMode={activation} className={cx("flex min-h-0 flex-col", className)}>
      <T.List
        aria-label={label}
        className={cx(
          "flex shrink-0 items-center overflow-x-auto",
          variant === "underline" ? "h-8 gap-4 border-b border-divider" : "gap-1 rounded-sm bg-surface-1 p-0.5",
        )}
      >
        {items.map((item) => (
          <T.Trigger
            key={item.id}
            value={item.id}
            disabled={item.disabled}
            className={cx(
              "inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap text-base text-fg-muted transition-colors duration-[var(--dur-base)]",
              "hover:text-fg focus-visible:shadow-focus focus-visible:outline-none disabled:opacity-40",
              variant === "underline"
                ? "-mb-px h-8 border-b-2 border-transparent data-[state=active]:border-accent data-[state=active]:text-fg"
                : "h-7 rounded-sm px-2.5 data-[state=active]:bg-surface-4 data-[state=active]:text-fg",
            )}
          >
            {item.icon && <span aria-hidden="true">{item.icon}</span>}
            {item.label}
            {item.count !== undefined && <span className="rounded-full bg-surface-4 px-1.5 text-2xs tabular-nums">{item.count}</span>}
          </T.Trigger>
        ))}
      </T.List>
      {items.map((item) => (
        <T.Content key={item.id} value={item.id} className="min-h-0 flex-1 focus-visible:outline-none">
          {renderPanel(item.id)}
        </T.Content>
      ))}
    </T.Root>
  );
}
