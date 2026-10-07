import { create } from "zustand";
import type { SettingKey } from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

type Values = Partial<Record<SettingKey, unknown>>;

type SettingsState = {
  values: Values;
  loaded: boolean;
  load: () => Promise<void>;
  /** Applies at once; reverts and returns false if the write fails. */
  set: (key: SettingKey, value: unknown) => Promise<boolean>;
  reset: (key: SettingKey) => Promise<void>;
};

export const useSettings = create<SettingsState>((set, get) => ({
  values: {},
  loaded: false,
  load: async () => {
    const values = await invoke<Values>("settings_get_all");
    set({ values, loaded: true });
  },
  set: async (key, value) => {
    const previous = get().values[key];
    set({ values: { ...get().values, [key]: value } });
    try {
      await invoke("settings_set", { key, value });
      return true;
    } catch {
      set({ values: { ...get().values, [key]: previous } });
      return false;
    }
  },
  reset: async (key) => {
    await invoke("settings_reset", { key });
    await get().load();
  },
}));

/** A typed read with a fallback for missing values. */
export function useSetting<T>(key: SettingKey, fallback: T): T {
  return useSettings((s) => (s.values[key] as T | undefined) ?? fallback);
}

export function watchSettings(): Promise<() => void> {
  return listen<{ key: SettingKey; value: unknown }>("settings:changed", ({ key, value }) => {
    const values = useSettings.getState().values;
    if (values[key] !== value) useSettings.setState({ values: { ...values, [key]: value } });
  });
}
