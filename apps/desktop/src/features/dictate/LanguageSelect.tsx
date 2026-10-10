import { Select } from "@tandem/ui";
import { t } from "../../i18n";
import { useDictate } from "../../stores/dictate";
import { useSetting, useSettings } from "../../stores/settings";

/** The languages Dictate offers for one model: its own list, or a common
 * set for multilingual Whisper. */
const COMMON = ["en", "de", "fr", "es", "it", "pt", "nl", "pl", "sv", "da", "fi", "cs", "uk", "ru", "ja", "ko", "zh", "hi", "tr", "ar"];

export function languageName(code: string): string {
  try {
    return new Intl.DisplayNames([navigator.language || "en"], { type: "language" }).of(code) ?? code;
  } catch {
    return code;
  }
}

/** "Auto-detect" first, then the active model's languages (UI-62). */
export function LanguageSelect() {
  const value = useSetting<string>("dictate_language", "auto");
  const models = useDictate((s) => s.models);
  const active = models?.models.find((m) => m.id === models.active);
  const langs = !active || active.languages.includes("*") ? COMMON : active.languages;
  const options = [
    { value: "auto", label: t("dictate.language.auto") },
    ...langs.map((code) => ({ value: code, label: languageName(code) })),
  ];
  if (value !== "auto" && !langs.includes(value))
    options.push({
      value,
      label: languageName(value),
      disabled: true,
      disabledReason: t("dictate.language.unsupported", { model: active?.name ?? "" }),
    } as (typeof options)[number]);
  return (
    <Select
      ariaLabel={t("dictate.language")}
      size="sm"
      options={options}
      value={value}
      onValueChange={(v) => void useSettings.getState().set("dictate_language", v)}
      className="w-56"
    />
  );
}
