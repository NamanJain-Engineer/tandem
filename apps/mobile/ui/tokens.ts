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
  bg: "#060a12",
  surface1: "#0a0f1a",
  surface2: "#0d1320",
  surface3: "#111827",
  surface4: "#172033",
  border: "rgb(148 163 184 / 0.14)",
  borderStrong: "rgb(148 163 184 / 0.26)",
  text: "#e6edf5",
  textMuted: "#9aa8bb",
  textSubtle: "#7d8ba0",
  textDisabled: "#4a5668",
  accent: "#2dd4bf",
  accentPress: "#14b8a6",
  accentFg: "#042f2c",
  accentSoft: "rgb(45 212 191 / 0.14)",
  working: "#38bdf8",
  waiting: "#fbbf24",
  done: "#4ade80",
  idle: "#64748b",
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
