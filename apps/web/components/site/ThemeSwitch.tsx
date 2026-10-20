import { Monitor, Moon, Sun, type LucideIcon } from "lucide-react";
import { cx } from "../../lib/cx";
import type { ThemeChoice } from "../../lib/theme";
import { Icon } from "./Icon";

const OPTIONS: ReadonlyArray<{ value: ThemeChoice; label: string; icon: LucideIcon }> = [
  { value: "system", label: "System", icon: Monitor },
  { value: "light", label: "Light", icon: Sun },
  { value: "dark", label: "Dark", icon: Moon },
];

/** System, Light or Dark (F-WB-01 AC4, D-1870): three radios that the theme
 * script in `<head>` answers (`data-theme-set`). Server-rendered, no script
 * of its own. The selected look comes from `data-theme-choice` on `<html>`
 * (site.css), so it's right at the first paint, before the radios are
 * checked. Shown only once that script has run: without JavaScript the
 * page follows the OS and a control that did nothing would mislead. `name`
 * keeps two on one page (the header and the phone menu) apart. */
export function ThemeSwitch({ name, labels = false, className }: { name: string; labels?: boolean; className?: string }) {
  return (
    <fieldset className={cx("theme-switch", labels && "theme-switch-wide", className)}>
      <legend className="sr-only">Theme</legend>
      {OPTIONS.map((o) => (
        <label key={o.value} data-choice={o.value} title={labels ? undefined : o.label}>
          <input type="radio" name={name} value={o.value} data-theme-set="" className="sr-only" />
          <Icon icon={o.icon} size={labels ? 16 : 15} />
          <span className={labels ? undefined : "sr-only"}>{o.label}</span>
        </label>
      ))}
    </fieldset>
  );
}
