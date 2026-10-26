import { Popover as P, Select as S } from "radix-ui";
import { Command } from "cmdk";
import { Check, ChevronDown, Plus } from "lucide-react";
import { useId, useState, type ReactNode } from "react";
import { cx } from "../cx";

export type SelectOption = {
  value: string;
  label: string;
  icon?: ReactNode;
  description?: string;
  disabled?: boolean;
  disabledReason?: string;
  group?: string;
};

const TRIGGER =
  "inline-flex w-full items-center gap-2 rounded-sm border border-border bg-surface-4 px-2.5 text-left text-base text-fg hover:border-border-strong focus-visible:border-accent focus-visible:shadow-focus focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-40";
const PANEL = "tn-pop z-[var(--z-popover)] min-w-[200px] overflow-hidden rounded-md border border-border bg-surface-3 p-1 shadow-e2";
const ITEM =
  "relative flex cursor-default select-none items-start gap-2 rounded-sm px-2 py-1 text-base text-fg outline-none data-[disabled]:opacity-40 data-[highlighted]:bg-surface-4 data-[selected=true]:bg-surface-4";

function groupsOf(options: SelectOption[]) {
  const groups = new Map<string, SelectOption[]>();
  for (const o of options) {
    const key = o.group ?? "";
    groups.set(key, [...(groups.get(key) ?? []), o]);
  }
  return [...groups.entries()];
}

export type SelectProps = {
  options: SelectOption[];
  value: string;
  onValueChange?: (value: string) => void;
  /** Accessible name (or use `label` for a visible one). */
  ariaLabel?: string;
  label?: string;
  placeholder?: string;
  size?: "sm" | "md";
  disabled?: boolean;
  id?: string;
  className?: string;
};

/** UI_SPEC §2.4: Radix Select styled like an Input with a chevron. */
export function Select({ options, value, onValueChange, ariaLabel, label, placeholder, size = "md", disabled, id, className }: SelectProps) {
  const auto = useId();
  const sid = id ?? auto;
  return (
    <div className={cx("flex flex-col gap-1.5", className)}>
      {label && (
        <label htmlFor={sid} className="text-sm font-medium text-fg">
          {label}
        </label>
      )}
      <S.Root value={value} onValueChange={onValueChange} disabled={disabled}>
        <S.Trigger id={sid} aria-label={label ? undefined : ariaLabel} className={cx(TRIGGER, size === "sm" ? "h-7 text-sm" : "h-8")}>
          <span className="min-w-0 flex-1 truncate">
            <S.Value placeholder={placeholder} />
          </span>
          <S.Icon>
            <ChevronDown size={14} aria-hidden="true" className="text-fg-subtle" />
          </S.Icon>
        </S.Trigger>
        <S.Portal>
          <S.Content position="popper" sideOffset={4} className={cx(PANEL, "w-[var(--radix-select-trigger-width)]")}>
            <S.Viewport>
              {groupsOf(options).map(([group, items], gi) => (
                <S.Group key={group || gi}>
                  {group && <S.Label className="px-2 pb-1 pt-2 text-2xs font-semibold uppercase tracking-caps text-fg-subtle">{group}</S.Label>}
                  {items.map((o) => (
                    <S.Item key={o.value} value={o.value} disabled={o.disabled} className={ITEM} title={o.disabled ? o.disabledReason : undefined}>
                      {o.icon && <span aria-hidden="true" className="mt-0.5">{o.icon}</span>}
                      <span className="flex min-w-0 flex-1 flex-col">
                        <S.ItemText>{o.label}</S.ItemText>
                        {(o.description || (o.disabled && o.disabledReason)) && (
                          <span className="text-sm text-fg-subtle">{o.disabled && o.disabledReason ? o.disabledReason : o.description}</span>
                        )}
                      </span>
                      <S.ItemIndicator>
                        <Check size={14} aria-hidden="true" className="mt-0.5 text-accent" />
                      </S.ItemIndicator>
                    </S.Item>
                  ))}
                </S.Group>
              ))}
            </S.Viewport>
          </S.Content>
        </S.Portal>
      </S.Root>
    </div>
  );
}

export type ComboboxProps = {
  options: SelectOption[];
  value: string;
  onValueChange?: (value: string) => void;
  ariaLabel: string;
  placeholder?: string;
  searchPlaceholder?: string;
  emptyText?: string;
  /** Adds an "Add {query}…" row. */
  onCreate?: (query: string) => void;
  createLabel?: (query: string) => string;
  disabled?: boolean;
  className?: string;
};

/** UI_SPEC §2.4: type-to-filter, optional creatable row (cmdk). */
export function Combobox({
  options,
  value,
  onValueChange,
  ariaLabel,
  placeholder = "Select…",
  searchPlaceholder = "Search…",
  emptyText = "No matches",
  onCreate,
  createLabel = (q) => `Add ${q}…`,
  disabled,
  className,
}: ComboboxProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const current = options.find((o) => o.value === value);
  return (
    <P.Root open={open} onOpenChange={setOpen}>
      <P.Trigger asChild disabled={disabled}>
        <button type="button" role="combobox" aria-expanded={open} aria-label={ariaLabel} className={cx(TRIGGER, "h-8", className)}>
          <span className={cx("min-w-0 flex-1 truncate", !current && "text-fg-subtle")}>{current?.label ?? placeholder}</span>
          <ChevronDown size={14} aria-hidden="true" className="text-fg-subtle" />
        </button>
      </P.Trigger>
      <P.Portal>
        <P.Content sideOffset={4} align="start" className={cx(PANEL, "w-[var(--radix-popover-trigger-width)] p-0")}>
          <Command label={ariaLabel}>
            <Command.Input
              value={query}
              onValueChange={setQuery}
              placeholder={searchPlaceholder}
              className="h-8 w-full border-b border-divider bg-transparent px-2.5 text-base text-fg placeholder:text-fg-subtle focus:outline-none"
            />
            <Command.List className="max-h-64 overflow-y-auto p-1">
              <Command.Empty className="px-2 py-1.5 text-sm text-fg-subtle">{emptyText}</Command.Empty>
              {options.map((o) => (
                <Command.Item
                  key={o.value}
                  value={o.label}
                  disabled={o.disabled}
                  onSelect={() => {
                    onValueChange?.(o.value);
                    setOpen(false);
                  }}
                  className={ITEM}
                >
                  {o.icon && <span aria-hidden="true">{o.icon}</span>}
                  <span className="flex-1 truncate">{o.label}</span>
                  {o.value === value && <Check size={14} aria-hidden="true" className="text-accent" />}
                </Command.Item>
              ))}
              {onCreate && query.trim() && !options.some((o) => o.label.toLowerCase() === query.trim().toLowerCase()) && (
                <Command.Item
                  value={`__create__${query}`}
                  onSelect={() => {
                    onCreate(query.trim());
                    setOpen(false);
                  }}
                  className={ITEM}
                >
                  <Plus size={14} aria-hidden="true" />
                  {createLabel(query.trim())}
                </Command.Item>
              )}
            </Command.List>
          </Command>
        </P.Content>
      </P.Portal>
    </P.Root>
  );
}
