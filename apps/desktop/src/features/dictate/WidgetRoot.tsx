import { useEffect, useState } from "react";
import type { DictateStatus } from "@tandem/protocol";
import { UiProvider, type ThemeSetting } from "@tandem/ui";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { DictateWidget } from "./Widget";

/** The widget window's root: the app's theme, nothing else of the app. */
export function WidgetRoot() {
  const [theme, setTheme] = useState<ThemeSetting>("dark");
  useEffect(() => {
    const apply = (s: DictateStatus) => setTheme((s.theme as ThemeSetting) || "dark");
    void invoke<DictateStatus>("dictate_status").then(apply, () => undefined);
    const off = listen<DictateStatus>("dictate:status", apply);
    return () => void off.then((f) => f());
  }, []);
  return (
    <UiProvider theme={theme}>
      <DictateWidget />
    </UiProvider>
  );
}
