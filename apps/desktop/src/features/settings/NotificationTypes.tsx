import { Play } from "lucide-react";
import { Checkbox, IconButton } from "@tandem/ui";
import { t } from "../../i18n";
import { useSetting } from "../../stores/settings";
import { playSound } from "../notifications/sound";
import { useSave } from "./controls";

/** "<type>.<channel>" → on (settings maps are flat). */
type Types = Record<string, boolean>;

/** UI-41 defaults: [system, sound]. */
const ROWS: Array<{ id: string; label: () => string; system: boolean; sound: boolean }> = [
  { id: "approval", label: () => t("settings.notify.approval"), system: true, sound: true },
  { id: "done", label: () => t("settings.notify.done"), system: true, sound: false },
  { id: "failed", label: () => t("settings.notify.failed"), system: true, sound: false },
  { id: "routines", label: () => t("settings.notify.routines"), system: true, sound: false },
  { id: "tasks", label: () => t("settings.notify.tasks"), system: false, sound: false },
  { id: "runaway", label: () => t("settings.notify.type.runaway"), system: true, sound: false },
  { id: "usage", label: () => t("settings.notify.type.usage"), system: true, sound: false },
];

/** "What to notify": in-app is always on; system and sound per type. */
export function NotificationTypes() {
  const types = useSetting<Types>("notify_types", {});
  const save = useSave();
  const set = (id: string, channel: "system" | "sound", on: boolean) =>
    void save("notify_types", { ...types, [`${id}.${channel}`]: on });
  return (
    <table className="w-full text-base" data-testid="notify-types">
      <thead>
        <tr className="text-2xs font-semibold uppercase tracking-caps text-fg-subtle">
          <th className="py-1 text-left font-semibold">{t("settings.notify.what")}</th>
          <th className="w-16 py-1 font-semibold">{t("settings.notify.inApp")}</th>
          <th className="w-16 py-1 font-semibold">{t("settings.notify.system")}</th>
          <th className="w-16 py-1 font-semibold">{t("settings.notify.sound")}</th>
        </tr>
      </thead>
      <tbody>
        {ROWS.map((r) => {
          const system = types[`${r.id}.system`] ?? r.system;
          const sound = types[`${r.id}.sound`] ?? r.sound;
          return (
            <tr key={r.id} className="border-t border-divider">
              <td className="py-1.5 text-fg">{r.label()}</td>
              <td className="text-center">
                <Checkbox
                  checked
                  disabled
                  label={<span className="sr-only">{t("settings.notify.inAppOf", { type: r.label() })}</span>}
                />
              </td>
              <td className="text-center" data-testid={`notify-${r.id}-system`}>
                <Checkbox
                  checked={system}
                  label={
                    <span className="sr-only">{t("settings.notify.systemOf", { type: r.label() })}</span>
                  }
                  onCheckedChange={(v) => set(r.id, "system", v)}
                />
              </td>
              <td className="text-center">
                <Checkbox
                  checked={sound}
                  label={<span className="sr-only">{t("settings.notify.soundOf", { type: r.label() })}</span>}
                  onCheckedChange={(v) => set(r.id, "sound", v)}
                />
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/** ▶ previews the chosen sound without blocking. */
export function SoundPreview() {
  const name = useSetting<string>("notify_sound_name", "chime");
  return (
    <IconButton
      size="sm"
      icon={<Play size={13} />}
      label={t("settings.notify.preview")}
      onClick={() => playSound(name)}
    />
  );
}
