import {
  forwardRef,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type InputHTMLAttributes,
  type ReactNode,
  type TextareaHTMLAttributes,
} from "react";
import { Eye, EyeOff, X } from "lucide-react";
import { cx } from "../cx";

type FieldShell = {
  label?: string;
  hint?: string;
  error?: string;
  /** Hide the label visually (still read by screen readers). */
  hideLabel?: boolean;
  className?: string;
};

const CONTROL =
  "w-full rounded-sm border border-border bg-surface-4 text-base text-fg placeholder:text-fg-subtle transition-colors duration-[var(--dur-instant)] hover:border-border-strong focus:border-accent focus:shadow-focus focus:outline-none disabled:cursor-not-allowed disabled:opacity-40";

function Shell({
  id,
  label,
  hint,
  error,
  hideLabel,
  className,
  children,
}: FieldShell & { id: string; children: ReactNode }) {
  return (
    <div className={cx("flex flex-col gap-1.5", className)}>
      {label && (
        <label htmlFor={id} className={cx("text-sm font-medium text-fg", hideLabel && "sr-only")}>
          {label}
        </label>
      )}
      {children}
      {error ? (
        <p id={`${id}-error`} className="text-sm text-danger">
          {error}
        </p>
      ) : hint ? (
        <p id={`${id}-hint`} className="text-sm text-fg-subtle">
          {hint}
        </p>
      ) : null}
    </div>
  );
}

export type InputProps = Omit<InputHTMLAttributes<HTMLInputElement>, "prefix" | "size"> &
  FieldShell & {
    prefix?: ReactNode;
    suffix?: ReactNode;
    clearable?: boolean;
    onClear?: () => void;
    monospace?: boolean;
    clearLabel?: string;
  };

/** UI_SPEC §2.3: 32px, surface-4 fill, accent focus, danger error with aria-describedby. */
export const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { label, hint, error, hideLabel, className, prefix, suffix, clearable, onClear, monospace, clearLabel = "Clear", id, ...rest },
  ref,
) {
  const autoId = useId();
  const inputId = id ?? autoId;
  const describedBy = error ? `${inputId}-error` : hint ? `${inputId}-hint` : undefined;
  const showClear = clearable && rest.value !== undefined && rest.value !== "";
  return (
    <Shell id={inputId} label={label} hint={hint} error={error} hideLabel={hideLabel} className={className}>
      <div className="relative flex items-center">
        {prefix && <span className="pointer-events-none absolute left-2.5 inline-flex text-fg-subtle">{prefix}</span>}
        <input
          ref={ref}
          id={inputId}
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy}
          className={cx(
            CONTROL,
            "h-8 px-2.5",
            prefix ? "pl-8" : null,
            suffix || showClear ? "pr-8" : null,
            monospace && "font-mono text-sm",
            error && "border-danger focus:border-danger",
          )}
          {...rest}
        />
        {showClear ? (
          <button
            type="button"
            aria-label={clearLabel}
            onClick={onClear}
            className="absolute right-1.5 inline-flex size-6 items-center justify-center rounded-xs text-fg-subtle hover:bg-surface-3 hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
          >
            <X size={14} aria-hidden="true" />
          </button>
        ) : (
          suffix && <span className="absolute right-2.5 inline-flex text-fg-subtle">{suffix}</span>
        )}
      </div>
    </Shell>
  );
});

export type SecretInputProps = Omit<InputProps, "type" | "suffix"> & { showLabel?: string; hideLabel2?: string };

/** Masked input with a show/hide toggle; the value is never logged. */
export const SecretInput = forwardRef<HTMLInputElement, SecretInputProps>(function SecretInput(
  { showLabel = "Show", hideLabel2 = "Hide", ...rest },
  ref,
) {
  const [shown, setShown] = useState(false);
  return (
    <div className="relative">
      <Input ref={ref} type={shown ? "text" : "password"} autoComplete="off" spellCheck={false} {...rest} className={cx("[&_input]:pr-9", rest.className)} />
      <button
        type="button"
        aria-label={shown ? hideLabel2 : showLabel}
        aria-pressed={shown}
        onClick={() => setShown((s) => !s)}
        className="absolute bottom-1 right-1.5 inline-flex size-6 items-center justify-center rounded-xs text-fg-subtle hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
      >
        {shown ? <EyeOff size={14} aria-hidden="true" /> : <Eye size={14} aria-hidden="true" />}
      </button>
    </div>
  );
});

export type TextareaProps = TextareaHTMLAttributes<HTMLTextAreaElement> &
  FieldShell & { minRows?: number; maxRows?: number; monospace?: boolean };

/** Autosizes between `minRows` and `maxRows`, then scrolls. */
export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(function Textarea(
  { label, hint, error, hideLabel, className, minRows = 3, maxRows = 12, monospace, id, value, ...rest },
  ref,
) {
  const autoId = useId();
  const areaId = id ?? autoId;
  const inner = useRef<HTMLTextAreaElement | null>(null);
  useLayoutEffect(() => {
    const el = inner.current;
    if (!el) return;
    const line = 20;
    el.style.height = "auto";
    const rows = Math.min(maxRows, Math.max(minRows, Math.ceil(el.scrollHeight / line)));
    el.style.height = `${rows * line + 12}px`;
  }, [value, minRows, maxRows]);
  return (
    <Shell id={areaId} label={label} hint={hint} error={error} hideLabel={hideLabel} className={className}>
      <textarea
        ref={(el) => {
          inner.current = el;
          if (typeof ref === "function") ref(el);
          else if (ref) ref.current = el;
        }}
        id={areaId}
        rows={minRows}
        value={value}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? `${areaId}-error` : hint ? `${areaId}-hint` : undefined}
        className={cx(CONTROL, "resize-none px-2.5 py-1.5", monospace && "font-mono text-sm", error && "border-danger")}
        {...rest}
      />
    </Shell>
  );
});
