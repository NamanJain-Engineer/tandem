import { useEffect, useState } from "react";
import type { GuardLimits } from "@tandem/protocol";
import { Checkbox, Input } from "@tandem/ui";
import { t } from "../../i18n";
import { useSetting } from "../../stores/settings";
import { useSave } from "./controls";

/** The defaults the core uses when the setting is missing (F-AC-09). */
export const GUARD_DEFAULTS: GuardLimits = {
  workingMinutes: 90,
  windowPoints: 30,
  subagents: 20,
  circles: true,
};

type NumKey = "workingMinutes" | "windowPoints" | "subagents";
const ROWS: Array<{ key: NumKey; label: () => string; min: number; max: number }> = [
  { key: "workingMinutes", label: () => t("settings.runaway.working"), min: 10, max: 1_440 },
  { key: "windowPoints", label: () => t("settings.runaway.window"), min: 5, max: 100 },
  { key: "subagents", label: () => t("settings.runaway.subagents"), min: 1, max: 500 },
];

/** Reads the setting's map with the defaults behind it. */
export function guardLimits(raw: Partial<GuardLimits> | null | undefined): GuardLimits {
  return { ...GUARD_DEFAULTS, ...(raw ?? {}) };
}

/** UI-100 Settings › Teammates › Runaway guard: each limit can be turned off. */
export function RunawayGuardControl() {
  const raw = useSetting<Partial<GuardLimits>>("runaway_guard", GUARD_DEFAULTS);
  const limits = guardLimits(raw);
  const save = useSave();
  const [drafts, setDrafts] = useState<Record<NumKey, string>>({
    workingMinutes: "",
    windowPoints: "",
    subagents: "",
  });
  useEffect(() => {
    setDrafts({
      workingMinutes: String(limits.workingMinutes ?? GUARD_DEFAULTS.workingMinutes),
      windowPoints: String(limits.windowPoints ?? GUARD_DEFAULTS.windowPoints),
      subagents: String(limits.subagents ?? GUARD_DEFAULTS.subagents),
    });
  }, [limits.workingMinutes, limits.windowPoints, limits.subagents]);
  const put = (patch: Partial<GuardLimits>) => void save("runaway_guard", { ...limits, ...patch });
  return (
    <div className="flex flex-col gap-2" data-testid="runaway-guard">
      {ROWS.map((r) => {
        const on = limits[r.key] !== null;
        return (
          <div key={r.key} className="flex items-center gap-2">
            <span data-testid={`runaway-${r.key}-on`}>
              <Checkbox
                checked={on}
                label={<span className="sr-only">{t("settings.runaway.on")}</span>}
                onCheckedChange={(v) => put({ [r.key]: v ? GUARD_DEFAULTS[r.key] : null })}
              />
            </span>
            <Input
              type="number"
              min={r.min}
              max={r.max}
              label={r.label()}
              className="w-24"
              disabled={!on}
              value={drafts[r.key]}
              onChange={(e) => setDrafts((d) => ({ ...d, [r.key]: e.target.value }))}
              onBlur={() => {
                const n = Number(drafts[r.key]);
                if (!Number.isFinite(n) || n < r.min || n > r.max) return;
                if (n !== limits[r.key]) put({ [r.key]: n });
              }}
            />
          </div>
        );
      })}
      <span data-testid="runaway-circles">
        <Checkbox
          checked={limits.circles}
          label={t("settings.runaway.circles")}
          onCheckedChange={(v) => put({ circles: v })}
        />
      </span>
    </div>
  );
}
