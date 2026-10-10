import { useEffect, useRef, useState } from "react";
import { cx } from "../cx";
import { useTheme } from "../theme/ThemeProvider";

export type WaveformState = "idle" | "listening" | "thinking" | "speaking" | "error";

export type WaveformRingProps = {
  state: WaveformState;
  /** 0–1, the latest mic (or output) level. */
  level?: number;
  /** 28 (compact), 56 (expanded) or 120 (onboarding). */
  size?: number;
  /** Announced politely when it changes ("Listening", "Transcribing"). */
  label: string;
  className?: string;
};

const BARS = 48;

/** Per-bar weights: a smooth, fixed spread so the ring breathes unevenly,
 * like a spectrum, from a single level. */
const WEIGHTS = Array.from({ length: BARS }, (_, i) => 0.55 + 0.45 * Math.abs(Math.sin(i * 1.7) * Math.cos(i * 0.43)));

const COLOR: Record<WaveformState, string> = {
  idle: "var(--tn-status-idle)",
  listening: "var(--tn-accent)",
  thinking: "var(--tn-status-idle)",
  speaking: "var(--tn-status-working)",
  error: "var(--tn-status-error)",
};

/** UI_SPEC §2.30: 48 radial bars around a circle. Listening bars follow the
 * level; thinking rotates one accent highlight; error is a static rose ring
 * with a gap at the top. Reduced motion: no rotation, and the ring's
 * thickness shows the level instead, updated at most every 150 ms. */
export function WaveformRing({ state, level = 0, size = 28, label, className }: WaveformRingProps) {
  const { reducedMotion } = useTheme();
  const [smooth, setSmooth] = useState(0);
  const target = useRef(level);
  target.current = state === "listening" || state === "speaking" ? level : 0;

  useEffect(() => {
    if (reducedMotion) {
      const id = setInterval(() => setSmooth(target.current), 150);
      return () => clearInterval(id);
    }
    let raf = 0;
    let current = 0;
    const step = () => {
      // Fast attack, slower release.
      const t = target.current;
      current += (t - current) * (t > current ? 0.5 : 0.15);
      setSmooth((s) => (Math.abs(s - current) > 0.004 ? current : s));
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [reducedMotion]);

  const r = size / 2;
  const inner = r * 0.52;
  const base = r * 0.08;
  const room = r * 0.4;
  const stroke = Math.max(1, size / 28 * 2);
  const color = COLOR[state];
  const bars = [];
  for (let i = 0; i < BARS; i++) {
    const angle = (i / BARS) * Math.PI * 2 - Math.PI / 2;
    // Error: a gap at the top.
    if (state === "error" && (i <= 2 || i >= BARS - 2)) continue;
    const len = reducedMotion ? base : base + smooth * (WEIGHTS[i] ?? 1) * room;
    const x1 = r + Math.cos(angle) * inner;
    const y1 = r + Math.sin(angle) * inner;
    const x2 = r + Math.cos(angle) * (inner + len);
    const y2 = r + Math.sin(angle) * (inner + len);
    bars.push(
      <line key={i} x1={x1} y1={y1} x2={x2} y2={y2} stroke={color} strokeWidth={stroke} strokeLinecap="round" />,
    );
  }
  return (
    <span className={cx("relative inline-flex shrink-0", className)} style={{ width: size, height: size }}>
      <svg
        width={size}
        height={size}
        viewBox={`0 0 ${size} ${size}`}
        role="img"
        aria-label={label}
        data-state={state}
        data-testid="waveform-ring"
      >
        {reducedMotion && (
          <circle
            cx={r}
            cy={r}
            r={inner + base}
            fill="none"
            stroke={color}
            strokeWidth={stroke + smooth * r * 0.3}
          />
        )}
        {!reducedMotion && bars}
        {state === "thinking" && !reducedMotion && (
          <g className="origin-center animate-[spin_1.2s_linear_infinite]" style={{ transformOrigin: "50% 50%" }}>
            <line
              x1={r}
              y1={r - inner}
              x2={r}
              y2={r - inner - base * 2.5}
              stroke="var(--tn-accent)"
              strokeWidth={stroke * 1.5}
              strokeLinecap="round"
            />
          </g>
        )}
      </svg>
      <span className="sr-only" aria-live={state === "error" ? "assertive" : "polite"}>
        {label}
      </span>
    </span>
  );
}
