import { cx } from "../../lib/cx";

/** Vibezeo's backdrop (D-1834): the two-ring mark drawn at page scale, in
 * hairlines, with light where the rings overlap and along their upper arcs.
 * Decorative, and drawn by the browser, so it costs no request. `id` keeps
 * the gradient ids unique when a page draws it twice. */
export function Orbit({
  id,
  className,
  turn = false,
  draw = false,
}: {
  id: string;
  className?: string;
  turn?: boolean;
  /** The two rings draw themselves in as the page opens (D-1842). */
  draw?: boolean;
}) {
  const r = 340;
  const cy = 380;
  const left = 600 - r / 2;
  const right = 600 + r / 2;
  const lit = `orbit-lit-${id}`;
  const fade = `orbit-fade-${id}`;
  return (
    <svg
      aria-hidden="true"
      focusable="false"
      viewBox="0 0 1200 760"
      preserveAspectRatio="xMidYMin slice"
      fill="none"
      className={cx("pointer-events-none select-none", className)}
    >
      <defs>
        <linearGradient id={lit} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" style={{ stopColor: "var(--site-ring-lit)" }} />
          <stop offset="0.45" style={{ stopColor: "var(--site-ring)" }} />
          <stop offset="1" style={{ stopColor: "var(--site-ring)", stopOpacity: 0 }} />
        </linearGradient>
        <radialGradient id={fade} cx="0.5" cy="0.3" r="0.6">
          <stop offset="0" style={{ stopColor: "var(--site-glow-a)" }} />
          <stop offset="1" style={{ stopColor: "var(--site-glow-a)", stopOpacity: 0 }} />
        </radialGradient>
      </defs>
      {/* the outer echo: the same pair, wider and fainter */}
      <g stroke="var(--site-ring)" strokeWidth="1" vectorEffect="non-scaling-stroke" opacity="0.6">
        <circle cx={left - 40} cy={cy} r={r + 180} />
        <circle cx={right + 40} cy={cy} r={r + 180} />
      </g>
      {/* the light in the lens where the rings overlap */}
      <ellipse cx="600" cy={cy - 40} rx={r * 0.55} ry={r * 0.9} fill={`url(#${fade})`} />
      <g className={turn ? "ring-turn" : undefined} stroke={`url(#${lit})`} strokeWidth="1.1">
        {/* pathLength: the draw-in's dash is a share of the ring, at any size */}
        <circle cx={left} cy={cy} r={r} pathLength={100} className={draw ? "ring-draw" : undefined} />
        <circle cx={right} cy={cy} r={r} pathLength={100} className={draw ? "ring-draw ring-draw-2" : undefined} />
      </g>
    </svg>
  );
}
