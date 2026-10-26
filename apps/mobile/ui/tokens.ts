// The design tokens Vibezeo Remote draws with: packages/ui's dark theme
// (UI_SPEC §1.2) as React Native values. `tokens.test.ts` checks every
// colour against packages/ui/src/styles/tokens.css, so they can't drift.

/** Each colour and the CSS custom property it mirrors. */
export const CSS_VARS = {
  bg: "--tn-bg",
  surface1: "--tn-surface-1",
  surface2: "--tn-surface-2",
  surface3: "--tn-surface-3",
  surface4: "--tn-surface-4",
  border: "--tn-border",
  borderStrong: "--tn-border-strong",
  text: "--tn-text",
  textMuted: "--tn-text-muted",
  textSubtle: "--tn-text-subtle",
  textDisabled: "--tn-text-disabled",
  accent: "--tn-accent",
  accentPress: "--tn-accent-press",
  accentFg: "--tn-accent-fg",
  accentSoft: "--tn-accent-soft",
  working: "--tn-status-working",
  waiting: "--tn-status-waiting",
  done: "--tn-status-done",
  idle: "--tn-status-idle",
  error: "--tn-status-error",
  danger: "--tn-danger",
  dangerFg: "--tn-danger-fg",
} as const;

export const color: Record<keyof typeof CSS_VARS, string> = {
  bg: "#09090b",
  surface1: "#0e0e10",
  surface2: "#121214",
  surface3: "#1c1c20",
  surface4: "#232327",
  border: "rgb(255 255 255 / 0.08)",
  borderStrong: "rgb(255 255 255 / 0.15)",
  text: "#ededef",
  textMuted: "#a3a3ac",
  textSubtle: "#8b8b95",
  textDisabled: "#55555e",
  accent: "#2dd4bf",
  accentPress: "#14b8a6",
  accentFg: "#042f2c",
  accentSoft: "rgb(45 212 191 / 0.14)",
  working: "#38bdf8",
  waiting: "#fbbf24",
  done: "#4ade80",
  idle: "#71717a",
  error: "#fb7185",
  danger: "#fb7185",
  dangerFg: "#2b0710",
};

/** Radius and spacing from the same file (8px panels, 6px controls). */
export const radius = { xs: 4, sm: 6, md: 8, lg: 12 } as const;
export const space = { 1: 4, 2: 8, 3: 12, 4: 16, 5: 20, 6: 24, 8: 32 } as const;
export const font = { sm: 13, base: 15, md: 17, lg: 20, xl: 24 } as const;

/** React Native wants `rgba()`; this turns `rgb(r g b / a)` into it. */
export function rn(value: string): string {
  const m = /^rgb\((\d+) (\d+) (\d+) \/ ([\d.]+)\)$/.exec(value);
  return m ? `rgba(${m[1]}, ${m[2]}, ${m[3]}, ${m[4]})` : value;
}
