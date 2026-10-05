import { Checkbox as C, RadioGroup as R, Switch as S } from "radix-ui";
import { Check, Minus } from "lucide-react";
import { useId, type ReactNode } from "react";
import { cx } from "../cx";

/** UI_SPEC §2.5: every choice has a visible label, and clicking it toggles. */
export type CheckboxProps = {
  label: ReactNode;
  checked: boolean | "indeterminate";
  onCheckedChange?: (checked: boolean) => void;
  disabled?: boolean;
  description?: string;
  className?: string;
  id?: string;
};

export function Checkbox({ label, checked, onCheckedChange, disabled, description, className, id }: CheckboxProps) {
  const auto = useId();
  const cid = id ?? auto;
  return (
    <div className={cx("flex items-start gap-2", className)}>
      <C.Root
        id={cid}
        checked={checked}
        disabled={disabled}
        onCheckedChange={(v) => onCheckedChange?.(v === true)}
        aria-describedby={description ? `${cid}-desc` : undefined}
        className={cx(
          "mt-0.5 inline-flex size-4 shrink-0 items-center justify-center rounded-xs border border-border-strong bg-surface-4",
          "focus-visible:shadow-focus focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-40",
          "data-[state=checked]:border-accent data-[state=checked]:bg-accent data-[state=checked]:text-accent-fg",
          "data-[state=indeterminate]:border-accent data-[state=indeterminate]:bg-accent data-[state=indeterminate]:text-accent-fg",
        )}
      >
        <C.Indicator>
          {checked === "indeterminate" ? <Minus size={12} aria-hidden="true" /> : <Check size={12} aria-hidden="true" />}
        </C.Indicator>
      </C.Root>
      <div className="flex flex-col">
        <label htmlFor={cid} className={cx("text-base text-fg", disabled && "opacity-40")}>
          {label}
        </label>
        {description && (
          <span id={`${cid}-desc`} className="text-sm text-fg-subtle">
            {description}
          </span>
        )}
      </div>
    </div>
  );
}

export type RadioItem = { value: string; label: ReactNode; description?: string; disabled?: boolean };

export type RadioGroupProps = {
  label: string;
  items: RadioItem[];
  value: string;
  onValueChange?: (value: string) => void;
  orientation?: "vertical" | "horizontal";
  className?: string;
};

export function RadioGroup({ label, items, value, onValueChange, orientation = "vertical", className }: RadioGroupProps) {
  const base = useId();
  return (
    <R.Root
      aria-label={label}
      value={value}
      onValueChange={onValueChange}
      orientation={orientation}
      className={cx("flex gap-2", orientation === "vertical" ? "flex-col" : "flex-row flex-wrap gap-4", className)}
    >
      {items.map((item) => {
        const id = `${base}-${item.value}`;
        return (
          <div key={item.value} className="flex items-start gap-2">
            <R.Item
              id={id}
              value={item.value}
              disabled={item.disabled}
              className={cx(
                "mt-0.5 inline-flex size-4 shrink-0 items-center justify-center rounded-full border border-border-strong bg-surface-4",
                "focus-visible:shadow-focus focus-visible:outline-none disabled:opacity-40 data-[state=checked]:border-accent",
              )}
            >
              <R.Indicator className="size-2 rounded-full bg-accent" />
            </R.Item>
            <div className="flex flex-col">
              <label htmlFor={id} className={cx("text-base text-fg", item.disabled && "opacity-40")}>
                {item.label}
              </label>
              {item.description && <span className="text-sm text-fg-subtle">{item.description}</span>}
            </div>
          </div>
        );
      })}
    </R.Root>
  );
}

export type SwitchProps = {
  checked: boolean;
  onCheckedChange?: (checked: boolean) => void;
  disabled?: boolean;
  /** Visible label; omit only when an outer `<label htmlFor>` names it. */
  label?: ReactNode;
  id?: string;
  className?: string;
  "aria-describedby"?: string;
  "aria-label"?: string;
};

/** 28×16 track, accent when on. */
export function Switch({ checked, onCheckedChange, disabled, label, id, className, ...aria }: SwitchProps) {
  const auto = useId();
  const sid = id ?? auto;
  const control = (
    <S.Root
      id={sid}
      checked={checked}
      disabled={disabled}
      onCheckedChange={onCheckedChange}
      aria-describedby={aria["aria-describedby"]}
      aria-label={aria["aria-label"]}
      className={cx(
        "relative inline-flex h-4 w-7 shrink-0 items-center rounded-full bg-surface-4 ring-1 ring-border-strong transition-colors duration-[var(--dur-fast)]",
        "focus-visible:shadow-focus focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-40 data-[state=checked]:bg-accent data-[state=checked]:ring-accent",
      )}
    >
      <S.Thumb className="block size-3 translate-x-0.5 rounded-full bg-fg-muted transition-transform duration-[var(--dur-fast)] data-[state=checked]:translate-x-[14px] data-[state=checked]:bg-accent-fg" />
    </S.Root>
  );
  if (!label) return <span className={className}>{control}</span>;
  return (
    <div className={cx("flex items-center gap-2", className)}>
      <label htmlFor={sid} className={cx("text-base text-fg", disabled && "opacity-40")}>
        {label}
      </label>
      {control}
    </div>
  );
}
