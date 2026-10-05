import { useId, type HTMLAttributes, type ReactNode } from "react";
import { AlertTriangle, CheckCircle2, Info, X, XCircle } from "lucide-react";
import { cx } from "../cx";
import { Button } from "./Button";

/** UI_SPEC §2.17: centred column with an icon tile, title, body and actions. */
export function EmptyState({
  icon,
  title,
  body,
  primary,
  secondary,
  learnMore,
  className,
}: {
  icon?: ReactNode;
  title: string;
  body?: ReactNode;
  primary?: ReactNode;
  secondary?: ReactNode;
  learnMore?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cx("mx-auto flex max-w-[420px] flex-col items-center gap-3 px-4 py-10 text-center", className)}>
      {icon && (
        <span aria-hidden="true" className="inline-flex size-10 items-center justify-center rounded-md bg-surface-4 text-fg-muted">
          {icon}
        </span>
      )}
      <h2 className="text-md font-semibold text-fg">{title}</h2>
      {body && <p className="text-base text-fg-muted">{body}</p>}
      {(primary || secondary) && (
        <div className="mt-1 flex flex-wrap items-center justify-center gap-2">
          {primary}
          {secondary}
        </div>
      )}
      {learnMore}
    </div>
  );
}

export type ListRowProps = Omit<HTMLAttributes<HTMLDivElement>, "title"> & {
  leading?: ReactNode;
  title: ReactNode;
  secondary?: ReactNode;
  trailing?: ReactNode;
  selected?: boolean;
  /** Visible tooltip with the full title when truncated. */
  fullTitle?: string;
  depth?: number;
};

/** 30px row (§3.3): leading slot, title, secondary text, trailing slot. */
export function ListRow({ leading, title, secondary, trailing, selected, fullTitle, depth = 0, className, ...rest }: ListRowProps) {
  return (
    <div
      className={cx(
        "group flex h-[30px] cursor-default select-none items-center gap-1.5 rounded-sm px-2 text-base text-fg outline-none",
        "hover:bg-surface-4 focus-visible:shadow-focus",
        selected && "bg-accent-soft hover:bg-accent-soft",
        className,
      )}
      style={depth ? { paddingLeft: 8 + depth * 12 } : undefined}
      {...rest}
    >
      {leading && <span className="inline-flex shrink-0 items-center">{leading}</span>}
      <span className="min-w-0 flex-1 truncate" title={fullTitle}>
        {title}
      </span>
      {secondary && <span className="shrink-0 truncate text-sm text-fg-subtle">{secondary}</span>}
      {trailing && <span className="inline-flex shrink-0 items-center gap-1">{trailing}</span>}
    </div>
  );
}

export function PageHeader({ title, subtitle, actions, tabs, className }: { title: ReactNode; subtitle?: ReactNode; actions?: ReactNode; tabs?: ReactNode; className?: string }) {
  return (
    <header className={cx("flex flex-col gap-3 pb-4", className)}>
      <div className="flex items-start gap-3">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <h1 className="text-lg font-semibold text-fg">{title}</h1>
          {subtitle && <p className="text-base text-fg-muted">{subtitle}</p>}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
      </div>
      {tabs}
    </header>
  );
}

export type SettingsRowProps = {
  label: string;
  description?: ReactNode;
  /** The control, given its id and describedby so the label is wired (§7.2). */
  control: (ids: { id: string; describedBy?: string }) => ReactNode;
  /** Accent-soft background for 2 s when matched by the Settings filter. */
  highlighted?: boolean;
  badge?: ReactNode;
  /** A "Saved" hint or an inline error beside the control. */
  status?: ReactNode;
  disabledReason?: ReactNode;
  className?: string;
  settingKey?: string;
};

/** Label + description left, control right, 56px min height (§2.31). */
export function SettingsRow({ label, description, control, highlighted, badge, status, disabledReason, className, settingKey }: SettingsRowProps) {
  const id = useId();
  const descId = description || disabledReason ? `${id}-desc` : undefined;
  return (
    <div
      data-setting={settingKey}
      data-highlighted={highlighted || undefined}
      className={cx(
        "flex min-h-14 items-center gap-6 border-b border-divider px-3 py-2.5 transition-colors duration-[var(--dur-slow)] last:border-b-0",
        highlighted && "rounded-sm bg-accent-soft",
        className,
      )}
    >
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex items-center gap-2">
          <label htmlFor={id} className="text-base font-medium text-fg">
            {label}
          </label>
          {badge}
        </div>
        {(description || disabledReason) && (
          <div id={descId} className="text-sm text-fg-subtle">
            {disabledReason ?? description}
          </div>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {status}
        {control({ id, describedBy: descId })}
      </div>
    </div>
  );
}

export function SettingsGroup({ label, children }: { label?: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-1.5" aria-label={label}>
      {label && <h2 className="px-3 text-2xs font-semibold uppercase tracking-caps text-fg-subtle">{label}</h2>}
      <div className="rounded-md border border-border bg-surface-2">{children}</div>
    </section>
  );
}

export type Tone = "info" | "warning" | "error" | "success";

const TONE_ICON = { info: Info, warning: AlertTriangle, error: XCircle, success: CheckCircle2 };
const TONE_TEXT = { info: "text-info", warning: "text-warning", error: "text-danger", success: "text-success" };
const TONE_BG = {
  info: "bg-working-soft",
  warning: "bg-waiting-soft",
  error: "bg-error-soft",
  success: "bg-done-soft",
};

/** Full-width 32px banner (§2.31, UI-133). `status`, or `alert` for errors. */
export function Banner({
  tone = "info",
  children,
  action,
  onDismiss,
  dismissLabel = "Dismiss",
  className,
}: {
  tone?: Tone;
  children: ReactNode;
  action?: { label: string; onClick: () => void };
  onDismiss?: () => void;
  dismissLabel?: string;
  className?: string;
}) {
  const Icon = TONE_ICON[tone];
  return (
    <div role={tone === "error" ? "alert" : "status"} className={cx("flex min-h-8 items-center gap-2 px-3 py-1 text-sm text-fg", TONE_BG[tone], className)}>
      <Icon size={14} aria-hidden="true" className={cx("shrink-0", TONE_TEXT[tone])} />
      <span className="min-w-0 flex-1">{children}</span>
      {action && (
        <Button size="sm" variant="ghost" onClick={action.onClick}>
          {action.label}
        </Button>
      )}
      {onDismiss && (
        <button
          type="button"
          aria-label={dismissLabel}
          onClick={onDismiss}
          className="inline-flex size-6 items-center justify-center rounded-xs text-fg-subtle hover:text-fg focus-visible:shadow-focus focus-visible:outline-none"
        >
          <X size={14} aria-hidden="true" />
        </button>
      )}
    </div>
  );
}

/** Boxed note inside forms and gates (§2.31, UI-122). */
export function Callout({ tone = "info", title, children, actions, className }: { tone?: Tone | "accent"; title?: ReactNode; children?: ReactNode; actions?: ReactNode; className?: string }) {
  const border = tone === "accent" ? "border-accent" : tone === "warning" ? "border-warning" : tone === "error" ? "border-danger" : "border-border";
  return (
    <div className={cx("flex flex-col gap-1.5 rounded-md border bg-surface-2 p-3", border, className)}>
      {title && <p className="text-base font-semibold text-fg">{title}</p>}
      {children && <div className="text-base text-fg-muted">{children}</div>}
      {actions && <div className="mt-1 flex flex-wrap gap-2">{actions}</div>}
    </div>
  );
}

/** Two-column definition list (§2.31). */
export function KeyValue({ items, className }: { items: Array<{ key: string; value: ReactNode }>; className?: string }) {
  return (
    <dl className={cx("grid grid-cols-[minmax(96px,auto)_1fr] gap-x-6 gap-y-2 text-base", className)}>
      {items.map((it) => (
        <div key={it.key} className="contents">
          <dt className="text-fg-muted">{it.key}</dt>
          <dd className="min-w-0 text-fg">{it.value}</dd>
        </div>
      ))}
    </dl>
  );
}

/** Onboarding steps indicator (§2.31). */
export function Stepper({ steps, current, label = "Progress" }: { steps: string[]; current: number; label?: string }) {
  return (
    <nav aria-label={label}>
      <ol className="flex items-center gap-2">
        {steps.map((s, i) => (
          <li key={s} aria-current={i === current ? "step" : undefined} className="flex items-center gap-2">
            <span
              className={cx(
                "inline-flex size-5 items-center justify-center rounded-full text-2xs font-semibold tabular-nums",
                i < current ? "bg-accent text-accent-fg" : i === current ? "border border-accent text-accent" : "border border-border-strong text-fg-subtle",
              )}
            >
              {i + 1}
            </span>
            <span className={cx("text-sm", i === current ? "text-fg" : "text-fg-muted")}>{s}</span>
            {i < steps.length - 1 && <span aria-hidden="true" className="h-px w-6 bg-border-strong" />}
          </li>
        ))}
      </ol>
    </nav>
  );
}
