import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import type { SettingKey } from "@tandem/protocol";
import { Checkbox, Combobox, Input, SegmentedControl, Select, Switch, cx, useToast } from "@tandem/ui";
import { t } from "../../i18n";
import { useSetting, useSettings } from "../../stores/settings";

type Ids = { id: string; describedBy?: string };

/** Writes a setting; a failure reverts the control and offers Retry (UI-100). */
export function useSave() {
  const set = useSettings((s) => s.set);
  const { show } = useToast();
  const save = async (key: SettingKey, value: unknown): Promise<boolean> => {
    const ok = await set(key, value);
    if (!ok)
      show({
        tone: "error",
        title: t("settings.saveFailed"),
        action: { label: t("settings.retry"), onClick: () => void save(key, value) },
      });
    return ok;
  };
  return save;
}

export function BoolControl({
  k,
  ids,
  label,
  disabled,
}: {
  k: SettingKey;
  ids: Ids;
  label: string;
  disabled?: boolean;
}) {
  const value = useSetting(k, false);
  const save = useSave();
  return (
    <Switch
      id={ids.id}
      aria-describedby={ids.describedBy}
      checked={value}
      disabled={disabled}
      aria-label={label}
      onCheckedChange={(v) => void save(k, v)}
    />
  );
}

const INTEGRATED_SHELLS = [
  ["bash", "Bash"],
  ["zsh", "Zsh"],
  ["fish", "Fish"],
  ["pwsh", "PowerShell"],
] as const;

/** Shell integration on or off per shell (F-CD-10); new terminals only. */
export function ShellIntegrationControl({ ids, label }: { ids: Ids; label: string }) {
  const value = useSetting<Record<string, boolean>>("term_shell_integration", {});
  const save = useSave();
  return (
    <div
      id={ids.id}
      role="group"
      aria-label={label}
      aria-describedby={ids.describedBy}
      className="flex flex-wrap gap-x-4 gap-y-2"
    >
      {INTEGRATED_SHELLS.map(([key, name]) => (
        <Checkbox
          key={key}
          label={name}
          checked={value[key] !== false}
          onCheckedChange={(on) => void save("term_shell_integration", { ...value, [key]: on })}
        />
      ))}
    </div>
  );
}

export function SelectControl({
  k,
  ids,
  options,
  fallback,
}: {
  k: SettingKey;
  ids: Ids;
  options: Array<{ value: string; label: string }>;
  fallback: string;
}) {
  const value = useSetting(k, fallback);
  const save = useSave();
  return (
    <div className="w-48">
      <Select id={ids.id} size="sm" value={value} options={options} onValueChange={(v) => void save(k, v)} />
    </div>
  );
}

export function SegmentControl({
  k,
  label,
  options,
  fallback,
}: {
  k: SettingKey;
  label: string;
  options: Array<{ id: string; label: string }>;
  fallback: string;
}) {
  const value = useSetting(k, fallback);
  const save = useSave();
  return (
    <SegmentedControl
      label={label}
      size="sm"
      value={value}
      items={options}
      onValueChange={(v) => void save(k, v)}
    />
  );
}

/** "Saved" for a second when the effect isn't visible (UI-100). */
function useSavedHint(): [boolean, () => void] {
  const [shown, setShown] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  return [
    shown,
    () => {
      setShown(true);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setShown(false), 1000);
    },
  ];
}

export function SavedHint({ shown }: { shown: boolean }) {
  return (
    <span
      role="status"
      className={cx("text-sm text-fg-subtle transition-opacity", shown ? "opacity-100" : "opacity-0")}
    >
      {shown ? t("settings.saved") : ""}
    </span>
  );
}

export function NumberControl({
  k,
  ids,
  min,
  max,
  fallback,
  label,
}: {
  k: SettingKey;
  ids: Ids;
  min: number;
  max: number;
  fallback: number;
  label: string;
}) {
  const value = useSetting(k, fallback);
  const save = useSave();
  const [draft, setDraft] = useState(String(value));
  const [saved, flash] = useSavedHint();
  useEffect(() => setDraft(String(value)), [value]);
  const commit = async () => {
    const n = Number(draft);
    if (!Number.isInteger(n) || n < min || n > max) {
      setDraft(String(value));
      return;
    }
    if (n !== value && (await save(k, n))) flash();
  };
  return (
    <span className="inline-flex items-center gap-2">
      <SavedHint shown={saved} />
      <Input
        id={ids.id}
        aria-describedby={ids.describedBy}
        aria-label={label}
        type="number"
        min={min}
        max={max}
        className="w-28"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={(e: KeyboardEvent) => e.key === "Enter" && void commit()}
      />
    </span>
  );
}

export function TextControl({
  k,
  ids,
  placeholder,
  label,
  monospace,
}: {
  k: SettingKey;
  ids: Ids;
  placeholder?: string;
  label: string;
  monospace?: boolean;
}) {
  const value = useSetting<string | null>(k, null);
  const save = useSave();
  const [draft, setDraft] = useState(value ?? "");
  const [saved, flash] = useSavedHint();
  useEffect(() => setDraft(value ?? ""), [value]);
  const commit = async () => {
    const next = draft.trim() === "" ? null : draft.trim();
    if (next !== value && (await save(k, next))) flash();
  };
  return (
    <span className="inline-flex items-center gap-2">
      <SavedHint shown={saved} />
      <Input
        id={ids.id}
        aria-describedby={ids.describedBy}
        aria-label={label}
        className="w-56"
        monospace={monospace}
        placeholder={placeholder}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={(e: KeyboardEvent) => e.key === "Enter" && void commit()}
      />
    </span>
  );
}

const FONTS = [
  "JetBrains Mono",
  "Menlo",
  "SF Mono",
  "Consolas",
  "Cascadia Mono",
  "DejaVu Sans Mono",
  "Ubuntu Mono",
  "Fira Code",
];

export function FontControl({ k, label }: { k: SettingKey; label: string }) {
  const value = useSetting(k, "JetBrains Mono");
  const save = useSave();
  const options = [...new Set([value, ...FONTS])].map((f) => ({ value: f, label: f }));
  return (
    <div className="w-48">
      <Combobox
        ariaLabel={label}
        value={value}
        options={options}
        onValueChange={(v) => void save(k, v)}
        onCreate={(v) => void save(k, v)}
      />
    </div>
  );
}

export function ThemeCards({ label }: { label: string }) {
  const value = useSetting("theme", "dark");
  const save = useSave();
  const cards: Array<{ id: string; label: string; swatch: ReactNode }> = [
    {
      id: "dark",
      label: t("settings.theme.dark"),
      swatch: <span className="block h-10 rounded-xs border border-border bg-[#060a12]" />,
    },
    {
      id: "light",
      label: t("settings.theme.light"),
      swatch: <span className="block h-10 rounded-xs border border-border bg-[#f1f5f9]" />,
    },
    {
      id: "system",
      label: t("settings.theme.system"),
      swatch: (
        <span className="block h-10 rounded-xs border border-border bg-[linear-gradient(135deg,#060a12_50%,#f1f5f9_50%)]" />
      ),
    },
  ];
  const onKey = (e: KeyboardEvent, i: number) => {
    const delta =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    if (!delta) return;
    e.preventDefault();
    const next = cards[(i + delta + cards.length) % cards.length];
    if (next) {
      void save("theme", next.id);
      (
        e.currentTarget.parentElement?.children[(i + delta + cards.length) % cards.length] as
          HTMLElement | undefined
      )?.focus();
    }
  };
  return (
    <div role="radiogroup" aria-label={label} className="flex gap-2">
      {cards.map((c, i) => (
        <button
          key={c.id}
          type="button"
          role="radio"
          aria-checked={value === c.id}
          tabIndex={value === c.id ? 0 : -1}
          onClick={() => void save("theme", c.id)}
          onKeyDown={(e) => onKey(e, i)}
          className={cx(
            "flex w-24 flex-col gap-1.5 rounded-md border p-1.5 text-sm text-fg focus-visible:shadow-focus focus-visible:outline-none",
            value === c.id ? "border-accent bg-accent-soft" : "border-border hover:border-border-strong",
          )}
        >
          {c.swatch}
          {c.label}
        </button>
      ))}
    </div>
  );
}
