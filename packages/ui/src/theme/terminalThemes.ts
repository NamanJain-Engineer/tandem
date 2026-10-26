/** UI_SPEC §1.9 terminal palettes (xterm `ITheme` shape). */
export type TerminalTheme = {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent: string;
  selectionBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
  /** Find highlights (#RRGGBB only: the search addon draws them as decorations). */
  searchMatch: string;
  searchActive: string;
};

export const tandemNight: TerminalTheme = {
  background: "#121214",
  foreground: "#e4e4e7",
  cursor: "#2dd4bf",
  cursorAccent: "#121214",
  selectionBackground: "rgba(45,212,191,0.28)",
  black: "#26262b",
  red: "#f87171",
  green: "#4ade80",
  yellow: "#fbbf24",
  blue: "#60a5fa",
  magenta: "#c084fc",
  cyan: "#2dd4bf",
  white: "#d4d4d8",
  brightBlack: "#5c5c66",
  brightRed: "#fca5a5",
  brightGreen: "#86efac",
  brightYellow: "#fde68a",
  brightBlue: "#93c5fd",
  brightMagenta: "#d8b4fe",
  brightCyan: "#5eead4",
  brightWhite: "#fafafa",
  searchMatch: "#3d3414",
  searchActive: "#8a5a0c",
};

export const tandemDay: TerminalTheme = {
  background: "#ffffff",
  foreground: "#27272a",
  cursor: "#0d9488",
  cursorAccent: "#ffffff",
  selectionBackground: "rgba(13,148,136,0.20)",
  black: "#27272a",
  red: "#dc2626",
  green: "#16a34a",
  yellow: "#b45309",
  blue: "#2563eb",
  magenta: "#9333ea",
  cyan: "#0d9488",
  white: "#71717a",
  brightBlack: "#52525b",
  brightRed: "#ef4444",
  brightGreen: "#22c55e",
  brightYellow: "#d97706",
  brightBlue: "#3b82f6",
  brightMagenta: "#a855f7",
  brightCyan: "#14b8a6",
  brightWhite: "#18181b",
  searchMatch: "#fde68a",
  searchActive: "#fbbf24",
};

/** Pure black/white with saturated ANSI. */
export const tandemHighContrast: TerminalTheme = {
  background: "#000000",
  foreground: "#ffffff",
  cursor: "#ffffff",
  cursorAccent: "#000000",
  selectionBackground: "rgba(255,255,255,0.35)",
  black: "#000000",
  red: "#ff3b3b",
  green: "#00e05a",
  yellow: "#ffd400",
  blue: "#3b8eff",
  magenta: "#ff4dff",
  cyan: "#00e5ff",
  white: "#e5e5e5",
  brightBlack: "#7f7f7f",
  brightRed: "#ff6b6b",
  brightGreen: "#5cff8f",
  brightYellow: "#ffff4d",
  brightBlue: "#6bb0ff",
  brightMagenta: "#ff8cff",
  brightCyan: "#6bf3ff",
  brightWhite: "#ffffff",
  searchMatch: "#4d4400",
  searchActive: "#0050c8",
};

export type TerminalThemeSetting = "follow_app" | "night" | "day" | "high_contrast";

export function terminalThemeFor(setting: TerminalThemeSetting, resolvedApp: "dark" | "light", highContrast = false): TerminalTheme {
  if (setting === "high_contrast" || (setting === "follow_app" && highContrast)) return tandemHighContrast;
  if (setting === "night") return tandemNight;
  if (setting === "day") return tandemDay;
  return resolvedApp === "light" ? tandemDay : tandemNight;
}
