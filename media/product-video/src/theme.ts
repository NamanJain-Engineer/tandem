import { Easing, interpolate, spring } from "remotion";

export const FPS = 30;
export const W = 1920;
export const H = 1080;
export const DURATION = Math.round(64.5 * FPS);

/** Vibezeo's dark theme tokens (packages/ui/src/styles/tokens.css). */
export const C = {
  bg: "#060a12",
  s1: "#0a0f1a",
  s2: "#0d1320",
  s3: "#111827",
  s4: "#172033",
  text: "#e6edf5",
  muted: "#9aa8bb",
  subtle: "#7d8ba0",
  accent: "#2dd4bf",
  accentHi: "#5eead4",
  accentFg: "#042f2c",
  sky: "#38bdf8",
  amber: "#fbbf24",
  green: "#4ade80",
  rose: "#fb7185",
  violet: "#a78bfa",
  border: "rgba(148,163,184,0.14)",
  borderStrong: "rgba(148,163,184,0.26)",
};

export const FONT = `"Inter", system-ui, sans-serif`;
export const MONO = `"JetBrains Mono", ui-monospace, monospace`;

export const sec = (s: number) => Math.round(s * FPS);

export const EXPO_OUT = Easing.bezier(0.16, 1, 0.3, 1);
export const EXPO_IN = Easing.bezier(0.7, 0, 0.84, 0);
export const IN_OUT = Easing.bezier(0.65, 0, 0.35, 1);

/** 0→1 over [from, from + dur] frames with an ease. */
export function ramp(frame: number, from: number, dur: number, ease = EXPO_OUT): number {
  return interpolate(frame, [from, from + dur], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: ease,
  });
}

export function pop(frame: number, at: number, damping = 14, stiffness = 170): number {
  return spring({ frame: frame - at, fps: FPS, config: { damping, stiffness, mass: 0.9 } });
}

/** Piecewise keyframes: [[frame, value], ...] eased between points. */
export function keys(frame: number, points: Array<[number, number]>, ease = IN_OUT): number {
  const f = points.map((p) => p[0]);
  const v = points.map((p) => p[1]);
  return interpolate(frame, f, v, { extrapolateLeft: "clamp", extrapolateRight: "clamp", easing: ease });
}
