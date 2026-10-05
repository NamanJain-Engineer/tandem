import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { TooltipProvider } from "../components/Tooltip";
import { ToastProvider } from "../components/Toast";

export type ThemeSetting = "dark" | "light" | "system";
export type MotionSetting = "system" | "on" | "off";
export type TextSize = "small" | "default" | "large";
export type Density = "compact" | "comfortable";

export type ThemeState = {
  resolved: "dark" | "light";
  reducedMotion: boolean;
  highContrast: boolean;
};

const ThemeContext = createContext<ThemeState>({ resolved: "dark", reducedMotion: false, highContrast: false });

function useMedia(query: string): boolean {
  const get = () => (typeof window !== "undefined" && typeof window.matchMedia === "function" ? window.matchMedia(query).matches : false);
  const [matches, setMatches] = useState(get);
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia(query);
    const on = () => setMatches(mq.matches);
    on();
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, [query]);
  return matches;
}

export type ThemeProviderProps = {
  theme?: ThemeSetting;
  motion?: MotionSetting;
  textSize?: TextSize;
  density?: Density;
  /** Element that carries the data attributes (default: <html>). */
  target?: HTMLElement;
  children: ReactNode;
};

/** Dark, light and system themes (follows the OS live), derived high
 * contrast and reduce motion (F-SH-05). Applies `data-theme`,
 * `data-contrast`, `data-motion`, `data-text-size` and `data-density`. */
export function ThemeProvider({ theme = "dark", motion = "system", textSize = "default", density = "compact", target, children }: ThemeProviderProps) {
  const osLight = useMedia("(prefers-color-scheme: light)");
  const osReduced = useMedia("(prefers-reduced-motion: reduce)");
  const osContrast = useMedia("(prefers-contrast: more)");
  const forced = useMedia("(forced-colors: active)");
  const resolved: "dark" | "light" = theme === "system" ? (osLight ? "light" : "dark") : theme;
  const reducedMotion = motion === "on" || (motion === "system" && osReduced);
  const highContrast = osContrast || forced;

  useEffect(() => {
    const el = target ?? document.documentElement;
    el.dataset.theme = resolved;
    el.dataset.motion = reducedMotion ? "reduced" : "full";
    el.dataset.textSize = textSize;
    el.dataset.density = density;
    if (highContrast) el.dataset.contrast = "more";
    else delete el.dataset.contrast;
  }, [target, resolved, reducedMotion, highContrast, textSize, density]);

  const value = useMemo(() => ({ resolved, reducedMotion, highContrast }), [resolved, reducedMotion, highContrast]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeState {
  return useContext(ThemeContext);
}

/** Everything components expect around them: theme, tooltips, toasts. */
export function UiProvider({ children, ...theme }: ThemeProviderProps) {
  return (
    <ThemeProvider {...theme}>
      <TooltipProvider>
        <ToastProvider>{children}</ToastProvider>
      </TooltipProvider>
    </ThemeProvider>
  );
}
