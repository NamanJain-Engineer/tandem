import type { CSSProperties, ReactNode } from "react";
import { AbsoluteFill, Img, OffthreadVideo, interpolate, random, staticFile, useCurrentFrame } from "remotion";
import { C, FONT, FPS, MONO, keys, pop, ramp } from "./theme";

/** Slow-moving glows, a fading grid, grain and a vignette. */
export function Backdrop({ hue = 0 }: { hue?: number }) {
  const f = useCurrentFrame();
  const t = f / FPS;
  const ax = 22 + Math.sin(t * 0.21) * 8;
  const ay = 28 + Math.cos(t * 0.17) * 7;
  const bx = 78 + Math.cos(t * 0.19) * 7;
  const by = 72 + Math.sin(t * 0.23) * 6;
  return (
    <AbsoluteFill style={{ background: C.bg, overflow: "hidden" }}>
      <AbsoluteFill
        style={{
          background: `radial-gradient(900px 640px at ${ax}% ${ay}%, rgba(45,212,191,${0.16 + hue * 0.04}), transparent 70%),
            radial-gradient(820px 620px at ${bx}% ${by}%, rgba(56,189,248,0.11), transparent 70%),
            radial-gradient(700px 500px at 50% 110%, rgba(167,139,250,0.07), transparent 70%)`,
        }}
      />
      <AbsoluteFill
        style={{
          backgroundImage:
            "linear-gradient(rgba(148,163,184,0.055) 1px, transparent 1px), linear-gradient(90deg, rgba(148,163,184,0.055) 1px, transparent 1px)",
          backgroundSize: "72px 72px",
          backgroundPosition: `${(t * 6) % 72}px ${(t * 10) % 72}px`,
          maskImage: "radial-gradient(ellipse 70% 60% at 50% 45%, black 20%, transparent 75%)",
          WebkitMaskImage: "radial-gradient(ellipse 70% 60% at 50% 45%, black 20%, transparent 75%)",
        }}
      />
    </AbsoluteFill>
  );
}

export function Grain() {
  const f = useCurrentFrame();
  return (
    <AbsoluteFill
      style={{
        backgroundImage: `url(${staticFile("grain.png")})`,
        backgroundPosition: `${Math.floor(random(`gx${f}`) * 512)}px ${Math.floor(random(`gy${f}`) * 512)}px`,
        mixBlendMode: "overlay",
        opacity: 0.07,
        pointerEvents: "none",
      }}
    />
  );
}

export function Vignette() {
  return (
    <AbsoluteFill
      style={{
        background: "radial-gradient(ellipse 85% 80% at 50% 50%, transparent 55%, rgba(0,0,0,0.55) 100%)",
        pointerEvents: "none",
      }}
    />
  );
}

/** Word-by-word reveal: rise, unblur, fade. */
export function Words({
  text,
  at,
  stagger = 3,
  style,
  accent = [],
  out,
}: {
  text: string;
  at: number;
  stagger?: number;
  style?: CSSProperties;
  accent?: string[];
  out?: number;
}) {
  const f = useCurrentFrame();
  const words = text.split(" ");
  const o = out === undefined ? 1 : 1 - ramp(f, out, 10);
  return (
    <div style={{ display: "flex", flexWrap: "wrap", gap: "0 0.28em", opacity: o, ...style }}>
      {words.map((w, i) => {
        const p = ramp(f, at + i * stagger, 18);
        const hot = accent.includes(w.replace(/[.,]/g, ""));
        return (
          <span
            key={`${w}${i}`}
            style={{
              display: "inline-block",
              opacity: p,
              transform: `translateY(${(1 - p) * 0.55}em)`,
              filter: `blur(${(1 - p) * 10}px)`,
              ...(hot
                ? {
                    background: `linear-gradient(90deg, ${C.accentHi}, ${C.sky})`,
                    WebkitBackgroundClip: "text",
                    backgroundClip: "text",
                    color: "transparent",
                  }
                : null),
            }}
          >
            {w}
          </span>
        );
      })}
    </div>
  );
}

export function Eyebrow({ children, at }: { children: ReactNode; at: number }) {
  const f = useCurrentFrame();
  const p = ramp(f, at, 16);
  return (
    <div
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 10,
        fontFamily: MONO,
        fontSize: 20,
        letterSpacing: "0.08em",
        textTransform: "uppercase",
        color: C.accent,
        opacity: p,
        transform: `translateX(${(1 - p) * -20}px)`,
      }}
    >
      <span style={{ width: 28 * p, height: 2, background: C.accent, display: "inline-block" }} />
      {children}
    </div>
  );
}

/** The two linked rings, drawn stroke by stroke. */
export function Logo({ size, at, glow = 1 }: { size: number; at: number; glow?: number }) {
  const f = useCurrentFrame();
  const circ = 2 * Math.PI * 7;
  const a = ramp(f, at, 22);
  const b = ramp(f, at + 5, 22);
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 28 28"
      fill="none"
      style={{ filter: `drop-shadow(0 0 ${18 * glow}px rgba(45,212,191,${0.55 * glow}))`, overflow: "visible" }}
    >
      <circle
        cx="10.5"
        cy="14"
        r="7"
        stroke={C.accent}
        strokeWidth="2.2"
        strokeDasharray={circ}
        strokeDashoffset={circ * (1 - a)}
        strokeLinecap="round"
        transform="rotate(-90 10.5 14)"
      />
      <circle
        cx="17.5"
        cy="14"
        r="7"
        stroke={C.accent}
        strokeWidth="2.2"
        strokeDasharray={circ}
        strokeDashoffset={circ * (1 - b)}
        strokeLinecap="round"
        transform="rotate(90 17.5 14)"
      />
    </svg>
  );
}

export type Zoom = [frame: number, scale: number, cx: number, cy: number];

/** Offset that brings focus point `c` (0–1) to the middle, without showing past the edges. */
function pan(size: number, z: number, c: number): number {
  const t = size / 2 - z * c * size;
  return Math.min(0, Math.max(size - z * size, t));
}

/**
 * A real recording of the app in a floating window. `zoom` keyframes are
 * local frames with the focus point in the recording's own 0–1 space.
 */
export function AppWindow({
  src,
  trimBefore = 0,
  rate = 1,
  width,
  aspect = 1.6,
  zoom = [[0, 1, 0.5, 0.5]],
  tiltY = [[0, 0]],
  tiltX = [[0, 0]],
  style,
  still,
  speedTag,
}: {
  src: string;
  trimBefore?: number;
  rate?: number;
  width: number;
  aspect?: number;
  zoom?: Zoom[];
  tiltY?: Array<[number, number]>;
  tiltX?: Array<[number, number]>;
  style?: CSSProperties;
  still?: boolean;
  speedTag?: boolean;
}) {
  const f = useCurrentFrame();
  const z = keys(f, zoom.map((k) => [k[0], k[1]]));
  const cx = keys(f, zoom.map((k) => [k[0], k[2]]));
  const cy = keys(f, zoom.map((k) => [k[0], k[3]]));
  const ry = keys(f, tiltY);
  const rx = keys(f, tiltX);
  const h = width / aspect;
  return (
    <div style={{ perspective: 2600, ...style }}>
      <div
        style={{
          width,
          height: h,
          borderRadius: 16,
          overflow: "hidden",
          position: "relative",
          transform: `rotateY(${ry}deg) rotateX(${rx}deg)`,
          transformStyle: "preserve-3d",
          boxShadow:
            "0 60px 140px rgba(0,0,0,0.65), 0 18px 40px rgba(0,0,0,0.45), 0 0 0 1px rgba(148,163,184,0.22), 0 0 80px rgba(45,212,191,0.10)",
          background: C.bg,
        }}
      >
        <div
          style={{
            position: "absolute",
            inset: 0,
            transformOrigin: "0 0",
            transform: `translate(${pan(width, z, cx)}px, ${pan(h, z, cy)}px) scale(${z})`,
          }}
        >
          {still ? (
            <Img src={staticFile(src)} style={{ width: "100%", height: "100%", objectFit: "cover" }} />
          ) : (
            <OffthreadVideo
              src={staticFile(src)}
              trimBefore={trimBefore}
              playbackRate={rate}
              muted
              style={{ width: "100%", height: "100%", objectFit: "cover" }}
            />
          )}
        </div>
        <div
          style={{
            position: "absolute",
            inset: 0,
            background: "linear-gradient(160deg, rgba(255,255,255,0.05), transparent 30%)",
            pointerEvents: "none",
          }}
        />
        {speedTag && rate > 1 ? (
          <div
            style={{
              position: "absolute",
              right: 14,
              bottom: 12,
              fontFamily: MONO,
              fontSize: 15,
              color: C.muted,
              background: "rgba(6,10,18,0.78)",
              border: `1px solid ${C.border}`,
              borderRadius: 6,
              padding: "3px 8px",
            }}
          >
            {`${rate}× speed`}
          </div>
        ) : null}
      </div>
    </div>
  );
}

/** A label pill that pops in at `at` (local frames). */
export function Callout({
  at,
  until,
  children,
  style,
  color = C.accent,
}: {
  at: number;
  until?: number;
  children: ReactNode;
  style?: CSSProperties;
  color?: string;
}) {
  const f = useCurrentFrame();
  const p = pop(f, at);
  const o = until === undefined ? 1 : 1 - ramp(f, until, 10);
  if (f < at) return null;
  return (
    <div
      style={{
        position: "absolute",
        display: "inline-flex",
        alignItems: "center",
        gap: 12,
        padding: "12px 20px",
        borderRadius: 999,
        background: "rgba(10,15,26,0.88)",
        border: `1px solid ${C.borderStrong}`,
        boxShadow: `0 20px 50px rgba(0,0,0,0.5), 0 0 0 4px rgba(45,212,191,${0.06 * p})`,
        backdropFilter: "blur(8px)",
        fontFamily: FONT,
        fontSize: 26,
        fontWeight: 600,
        color: C.text,
        transform: `scale(${0.6 + 0.4 * p}) translateY(${(1 - p) * 20}px)`,
        opacity: Math.min(p * 1.4, 1) * o,
        whiteSpace: "nowrap",
        ...style,
      }}
    >
      <span
        style={{
          width: 10,
          height: 10,
          borderRadius: 99,
          background: color,
          boxShadow: `0 0 12px ${color}`,
        }}
      />
      {children}
    </div>
  );
}

export function KeyCap({ label, at, pressAt }: { label: string; at: number; pressAt?: number }) {
  const f = useCurrentFrame();
  const p = pop(f, at);
  const press = pressAt === undefined ? 0 : interpolate(f - pressAt, [0, 3, 9], [0, 1, 0], { extrapolateLeft: "clamp", extrapolateRight: "clamp" });
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        minWidth: 64,
        height: 64,
        padding: "0 18px",
        borderRadius: 12,
        fontFamily: FONT,
        fontWeight: 600,
        fontSize: 26,
        color: C.text,
        background: `linear-gradient(180deg, ${C.s4}, ${C.s3})`,
        border: `1px solid ${C.borderStrong}`,
        boxShadow: `0 ${6 - press * 4}px 0 #04070d, 0 10px 30px rgba(0,0,0,0.45)${press ? `, 0 0 24px rgba(45,212,191,${0.5 * press})` : ""}`,
        transform: `translateY(${press * 4}px) scale(${0.5 + 0.5 * p})`,
        opacity: Math.min(1, p * 1.5),
      }}
    >
      {label}
    </span>
  );
}

export function Fade({ children, inAt = 0, outAt, dur = 10 }: { children: ReactNode; inAt?: number; outAt?: number; dur?: number }) {
  const f = useCurrentFrame();
  const i = ramp(f, inAt, dur);
  const o = outAt === undefined ? 1 : 1 - ramp(f, outAt, dur);
  return <AbsoluteFill style={{ opacity: i * o }}>{children}</AbsoluteFill>;
}

export const titleStyle: CSSProperties = {
  fontFamily: FONT,
  fontWeight: 700,
  fontSize: 76,
  lineHeight: 1.06,
  letterSpacing: "-0.035em",
  color: C.text,
};

export const subStyle: CSSProperties = {
  fontFamily: FONT,
  fontWeight: 450,
  fontSize: 30,
  lineHeight: 1.4,
  color: C.muted,
  letterSpacing: "-0.01em",
};
