import type { ReactNode } from "react";
import { AbsoluteFill, interpolate, random, useCurrentFrame } from "remotion";
import { APP, APP_ASPECT, CLIPS } from "./clips";
import { AppWindow, Callout, Eyebrow, KeyCap, Logo, Words, subStyle, titleStyle } from "./components";
import { C, EXPO_IN, FONT, FPS, MONO, pop, ramp, sec } from "./theme";

/** Every scene eases in from a soft zoom-blur and out the same way. */
function Scene({ children, len, inDur = 14, outDur = 10 }: { children: ReactNode; len: number; inDur?: number; outDur?: number }) {
  const f = useCurrentFrame();
  const i = ramp(f, 0, inDur);
  const o = ramp(f, len - outDur, outDur, EXPO_IN);
  return (
    <AbsoluteFill
      style={{
        opacity: i * (1 - o),
        transform: `scale(${0.96 + 0.04 * i + 0.06 * o})`,
        filter: `blur(${(1 - i) * 12 + o * 14}px)`,
      }}
    >
      {children}
    </AbsoluteFill>
  );
}

const fr = (s: number) => Math.round(s * FPS);

// ---------------------------------------------------------------- 1. Hook
const AGENTS = [
  { name: "Claude Code", x: 300, y: 250, c: C.amber },
  { name: "Codex", x: 1420, y: 210, c: C.text },
  { name: "Gemini CLI", x: 1230, y: 760, c: C.sky },
  { name: "Aider", x: 420, y: 770, c: C.green },
  { name: "Cursor Agent", x: 820, y: 150, c: C.violet },
  { name: "Copilot CLI", x: 1600, y: 520, c: C.text },
  { name: "OpenCode", x: 190, y: 520, c: C.accent },
  { name: "Amp", x: 900, y: 880, c: C.rose },
  { name: "Droid", x: 1620, y: 860, c: C.amber },
  { name: "Grok", x: 700, y: 420, c: C.sky },
];
const AGENT_AT = [0, 15, 30, 45, 60, 67, 75, 82, 90, 97];

function MiniTerm({ name, color, at, i }: { name: string; color: string; at: number; i: number }) {
  const f = useCurrentFrame();
  const p = pop(f, at, 12, 200);
  if (f < at) return null;
  const blink = Math.floor((f - at) / 8) % 2 === 0;
  const lines = 2 + (i % 3);
  return (
    <div
      style={{
        width: 330,
        borderRadius: 12,
        background: "rgba(13,19,32,0.92)",
        border: `1px solid ${C.borderStrong}`,
        boxShadow: "0 30px 70px rgba(0,0,0,0.55)",
        transform: `scale(${0.4 + 0.6 * p}) rotate(${(1 - p) * (i % 2 ? 8 : -8)}deg)`,
        opacity: Math.min(1, p * 1.6),
        overflow: "hidden",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "10px 12px", borderBottom: `1px solid ${C.border}` }}>
        {["#fb7185", "#fbbf24", "#4ade80"].map((d) => (
          <span key={d} style={{ width: 10, height: 10, borderRadius: 9, background: d, opacity: 0.8 }} />
        ))}
        <span style={{ marginLeft: 8, fontFamily: FONT, fontSize: 17, fontWeight: 600, color: C.muted }}>{name}</span>
      </div>
      <div style={{ padding: "12px 14px", fontFamily: MONO, fontSize: 16, color: C.subtle, display: "flex", flexDirection: "column", gap: 7 }}>
        {Array.from({ length: lines }, (_, k) => (
          <div key={k} style={{ height: 9, width: `${40 + random(`${name}${k}`) * 55}%`, borderRadius: 4, background: "rgba(148,163,184,0.16)" }} />
        ))}
        <div style={{ color }}>
          {"› "}
          <span style={{ opacity: blink ? 1 : 0 }}>▍</span>
        </div>
      </div>
    </div>
  );
}

export function Hook() {
  const f = useCurrentFrame();
  const len = fr(4);
  const shake = interpolate(f, [55, 100], [0, 9], { extrapolateLeft: "clamp", extrapolateRight: "clamp" });
  const sx = (random(`sx${f}`) - 0.5) * shake;
  const sy = (random(`sy${f}`) - 0.5) * shake;
  const suck = ramp(f, 102, 18, EXPO_IN);
  return (
    <AbsoluteFill style={{ transform: `translate(${sx}px, ${sy}px)` }}>
      {AGENTS.map((a, i) => {
        const dx = (960 - a.x) * suck;
        const dy = (540 - a.y) * suck;
        return (
          <div
            key={a.name}
            style={{
              position: "absolute",
              left: a.x - 165,
              top: a.y - 70,
              transform: `translate(${dx}px, ${dy}px) scale(${1 - suck * 0.85})`,
              filter: `blur(${suck * 10}px)`,
              opacity: 1 - suck,
            }}
          >
            <MiniTerm name={a.name} color={a.c} at={AGENT_AT[i] ?? 0} i={i} />
          </div>
        );
      })}
      <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", opacity: 1 - suck }}>
        <div
          style={{
            padding: "26px 44px",
            borderRadius: 24,
            background: f >= 60 ? "rgba(6,10,18,0.78)" : "transparent",
            backdropFilter: f >= 60 ? "blur(14px)" : undefined,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: 6,
          }}
        >
          <Words text="Too many agents." at={60} style={{ ...titleStyle, fontSize: 84 }} />
          <Words text="Too many terminals." at={78} style={{ ...titleStyle, fontSize: 84, color: C.muted }} />
        </div>
      </AbsoluteFill>
      <AbsoluteFill style={{ background: "#fff", opacity: interpolate(f, [len - 4, len], [0, 0.18], { extrapolateLeft: "clamp", extrapolateRight: "clamp" }) }} />
    </AbsoluteFill>
  );
}

// ---------------------------------------------------------------- 2. Logo
export function LogoReveal() {
  const f = useCurrentFrame();
  const len = fr(4);
  const flash = 1 - ramp(f, 0, 16);
  const ring = ramp(f, 0, 40);
  const out = ramp(f, len - 12, 12, EXPO_IN);
  const word = "Vibezeo";
  return (
    <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", transform: `scale(${1 + out * 0.12})`, opacity: 1 - out, filter: `blur(${out * 10}px)` }}>
      <div
        style={{
          position: "absolute",
          width: 300 + ring * 1500,
          height: 300 + ring * 1500,
          borderRadius: "50%",
          border: `2px solid rgba(45,212,191,${0.5 * (1 - ring)})`,
          boxShadow: `0 0 60px rgba(45,212,191,${0.3 * (1 - ring)})`,
        }}
      />
      <AbsoluteFill style={{ background: "radial-gradient(circle at 50% 45%, rgba(94,234,212,0.55), transparent 45%)", opacity: flash }} />
      <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 18, marginTop: -40 }}>
        <div style={{ transform: `scale(${0.6 + 0.4 * pop(f, 0, 11, 160)})` }}>
          <Logo size={190} at={0} glow={1.2} />
        </div>
        <div style={{ display: "flex", ...titleStyle, fontSize: 150, letterSpacing: "-0.05em", marginTop: -10 }}>
          {word.split("").map((ch, i) => {
            const p = ramp(f, 8 + i * 2.2, 18);
            return (
              <span key={i} style={{ display: "inline-block", opacity: p, transform: `translateY(${(1 - p) * 50}px)`, filter: `blur(${(1 - p) * 12}px)` }}>
                {ch}
              </span>
            );
          })}
        </div>
        <Words
          text="A desktop workspace for the coding agents you already use."
          at={30}
          stagger={2}
          accent={["coding", "agents"]}
          style={{ ...subStyle, fontSize: 40, justifyContent: "center", color: C.muted }}
        />
        <div
          style={{
            marginTop: 18,
            display: "inline-flex",
            alignItems: "center",
            gap: 10,
            padding: "10px 20px",
            borderRadius: 999,
            border: `1px solid rgba(45,212,191,0.35)`,
            background: "rgba(45,212,191,0.10)",
            color: C.accentHi,
            fontFamily: FONT,
            fontWeight: 600,
            fontSize: 24,
            opacity: ramp(f, 55, 16),
            transform: `translateY(${(1 - ramp(f, 55, 16)) * 16}px)`,
          }}
        >
          <span style={{ width: 9, height: 9, borderRadius: 9, background: C.accent, boxShadow: `0 0 10px ${C.accent}` }} />
          Released · 0.1.0 for Linux
        </div>
      </div>
    </AbsoluteFill>
  );
}

// ---------------------------------------------------------------- 3. Detect
export function Detect({ zoomFocus = [0.5, 0.545] }: { zoomFocus?: [number, number] }) {
  const f = useCurrentFrame();
  const len = fr(6);
  const slide = pop(f, 0, 16, 120);
  return (
    <Scene len={len}>
      <div style={{ position: "absolute", left: 120, top: 330, width: 640, display: "flex", flexDirection: "column", gap: 26 }}>
        <Eyebrow at={4}>Launch</Eyebrow>
        <Words text="Finds the agents you already use." at={8} accent={["agents"]} style={{ ...titleStyle }} />
        <Words
          text="Claude Code, Codex, Gemini CLI and more. Each one signs in its own way."
          at={26}
          stagger={1.5}
          style={subStyle}
        />
      </div>
      <div style={{ position: "absolute", left: 800 + (1 - slide) * 500, top: 215, opacity: Math.min(1, slide * 1.5) }}>
        <AppWindow
          src={APP}
          trimBefore={fr(CLIPS.launchGrid.from)}
          rate={CLIPS.launchGrid.rate}
          width={1040}
          aspect={APP_ASPECT}
          tiltY={[
            [0, -22],
            [30, -9],
            [150, -6],
          ]}
          tiltX={[
            [0, 6],
            [30, 2],
          ]}
          zoom={[
            [0, 1, 0.5, 0.5],
            [55, 1, 0.5, 0.5],
            [85, 1.85, zoomFocus[0], zoomFocus[1]],
            [150, 1.85, zoomFocus[0], zoomFocus[1]],
            [175, 1.1, 0.5, 0.5],
          ]}
        />
        <Callout at={fr(1.0)} until={fr(2.3)} style={{ left: 40, top: -36 }}>
          An empty tab lists every agent on your PATH
        </Callout>
        <Callout at={fr(2.2)} until={fr(5.2)} style={{ left: 220, top: 590 }} color={C.amber}>
          Claude Code, detected. One click to start it.
        </Callout>
      </div>
    </Scene>
  );
}

// ---------------------------------------------------------------- 4. Terminals
export type Beat = { at: number; label: string; keys?: string[] };

export function Terminals({
  beats,
  zoom,
}: {
  beats: Beat[];
  zoom: Array<[number, number, number, number]>;
}) {
  const f = useCurrentFrame();
  const len = fr(14);
  return (
    <Scene len={len}>
      <div style={{ position: "absolute", top: 54, left: 0, right: 0, display: "flex", flexDirection: "column", alignItems: "center", gap: 8 }}>
        <Words text="Real terminals, side by side." at={2} accent={["side", "by", "side."]} style={{ ...titleStyle, fontSize: 58, justifyContent: "center" }} />
      </div>
      <div style={{ position: "absolute", left: (1920 - 1360) / 2, top: 168 }}>
        <AppWindow
          src={APP}
          trimBefore={fr(CLIPS.terminals.from)}
          rate={CLIPS.terminals.rate}
          width={1360}
          aspect={APP_ASPECT}
          zoom={zoom}
          tiltX={[
            [0, 10],
            [24, 0],
          ]}
          speedTag
        />
      </div>
      {beats.map((b, i) => {
        const next = beats[i + 1]?.at ?? len - 12;
        const p = pop(f, b.at);
        if (f < b.at || f > next + 8) return null;
        const o = 1 - ramp(f, next - 2, 8);
        return (
          <div
            key={b.label}
            style={{
              position: "absolute",
              left: 80,
              bottom: 70,
              display: "flex",
              alignItems: "center",
              gap: 14,
              opacity: o,
              transform: `translateY(${(1 - p) * 30}px)`,
            }}
          >
            {(b.keys ?? []).map((k, j) => (
              <KeyCap key={`${k}${j}`} label={k} at={b.at + j * 3} pressAt={b.at + 8} />
            ))}
            <div
              style={{
                marginLeft: 8,
                padding: "14px 22px",
                borderRadius: 14,
                background: "rgba(10,15,26,0.9)",
                border: `1px solid ${C.borderStrong}`,
                fontFamily: FONT,
                fontWeight: 600,
                fontSize: 28,
                color: C.text,
                boxShadow: "0 20px 50px rgba(0,0,0,0.5)",
                opacity: Math.min(1, p * 1.5),
              }}
            >
              {b.label}
            </div>
          </div>
        );
      })}
    </Scene>
  );
}

// ---------------------------------------------------------------- 5. Live status (a diagram, not a recording)
const CARDS = [
  { name: "Claude Code", folder: "api", c: C.amber },
  { name: "Codex", folder: "web", c: C.text },
  { name: "Gemini CLI", folder: "docs", c: C.sky },
  { name: "Claude Code", folder: "mobile", c: C.amber },
];

function StatusPill({ state }: { state: "working" | "waiting" | "done" }) {
  const f = useCurrentFrame();
  const map = {
    working: { label: "Working", c: C.sky, bg: "rgba(56,189,248,0.14)" },
    waiting: { label: "Needs you", c: C.amber, bg: "rgba(251,191,36,0.16)" },
    done: { label: "Done", c: C.green, bg: "rgba(74,222,128,0.14)" },
  }[state];
  const pulse = state === "waiting" ? 0.5 + 0.5 * Math.sin(f / 4) : 0;
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 9,
        padding: "7px 14px",
        borderRadius: 999,
        background: map.bg,
        color: map.c,
        fontFamily: FONT,
        fontWeight: 600,
        fontSize: 20,
        boxShadow: state === "waiting" ? `0 0 0 ${4 + pulse * 6}px rgba(251,191,36,${0.12 * (1 - pulse)})` : undefined,
      }}
    >
      {state === "working" ? (
        <span
          style={{
            width: 12,
            height: 12,
            borderRadius: 12,
            border: `2px solid ${map.c}`,
            borderTopColor: "transparent",
            transform: `rotate(${f * 18}deg)`,
          }}
        />
      ) : state === "done" ? (
        <span style={{ fontSize: 18, lineHeight: 1 }}>✓</span>
      ) : (
        <span style={{ width: 10, height: 10, borderRadius: 10, background: map.c }} />
      )}
      {map.label}
    </span>
  );
}

export function Status() {
  const f = useCurrentFrame();
  const len = fr(6);
  const cardAt = [30, 48, 66, 84];
  const states: Array<"working" | "waiting" | "done"> = [
    "working",
    f >= 105 ? "waiting" : "working",
    f >= 135 ? "done" : "working",
    "working",
  ];
  const toast = pop(f, 157, 15, 150);
  return (
    <Scene len={len} outDur={6}>
      <div style={{ position: "absolute", left: 130, top: 300, width: 690, display: "flex", flexDirection: "column", gap: 26 }}>
        <Eyebrow at={4}>Live status</Eyebrow>
        <Words text="Know which agent needs you." at={8} accent={["needs", "you."]} style={titleStyle} />
        <Words
          text="Claude Code's own hooks report when a session works, needs you or finishes. The Dashboard shows every agent at once."
          at={26}
          stagger={1.2}
          style={subStyle}
        />
      </div>
      <div style={{ position: "absolute", left: 900, top: 230, display: "grid", gridTemplateColumns: "420px 420px", gap: 26 }}>
        {CARDS.map((c, i) => {
          const p = pop(f, cardAt[i] ?? 0, 13, 170);
          const s = states[i] ?? "working";
          const hot = s === "waiting";
          return (
            <div
              key={`${c.name}${c.folder}`}
              style={{
                height: 270,
                borderRadius: 18,
                background: C.s2,
                border: `1px solid ${hot ? "rgba(251,191,36,0.55)" : C.borderStrong}`,
                boxShadow: hot ? "0 0 0 1px rgba(251,191,36,0.3), 0 30px 80px rgba(251,191,36,0.12)" : "0 30px 70px rgba(0,0,0,0.5)",
                transform: `scale(${0.6 + 0.4 * p}) translateY(${(1 - p) * 40}px)`,
                opacity: Math.min(1, p * 1.5),
                padding: 24,
                display: "flex",
                flexDirection: "column",
                gap: 18,
              }}
            >
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                  <span style={{ fontFamily: FONT, fontWeight: 700, fontSize: 26, color: C.text }}>{c.name}</span>
                  <span style={{ fontFamily: MONO, fontSize: 17, color: C.subtle }}>{`~/${c.folder}`}</span>
                </div>
                <StatusPill state={s} />
              </div>
              {[0.9, 0.7, 0.82, 0.55].map((w, k) => (
                <div
                  key={k}
                  style={{
                    height: 11,
                    width: `${w * 100}%`,
                    borderRadius: 6,
                    background: `linear-gradient(90deg, rgba(148,163,184,0.10), rgba(148,163,184,${s === "working" ? 0.22 : 0.14}) ${((f * 3 + k * 40) % 200) - 50}%, rgba(148,163,184,0.10))`,
                  }}
                />
              ))}
            </div>
          );
        })}
      </div>
      <div
        style={{
          position: "absolute",
          right: 70,
          top: 60,
          width: 470,
          padding: "18px 22px",
          borderRadius: 16,
          background: "rgba(17,24,39,0.96)",
          border: `1px solid ${C.borderStrong}`,
          boxShadow: "0 30px 80px rgba(0,0,0,0.6)",
          transform: `translateX(${(1 - toast) * 560}px)`,
          display: "flex",
          gap: 16,
          alignItems: "center",
        }}
      >
        <Logo size={40} at={-30} glow={0.4} />
        <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
          <span style={{ fontFamily: FONT, fontWeight: 700, fontSize: 22, color: C.text }}>Codex needs you</span>
          <span style={{ fontFamily: FONT, fontSize: 19, color: C.muted }}>~/web · waiting for your answer</span>
        </div>
      </div>
      <div style={{ position: "absolute", left: 900, bottom: 44, fontFamily: FONT, fontSize: 18, color: C.subtle, opacity: ramp(f, 40, 20) }}>
        Illustration
      </div>
    </Scene>
  );
}

// ---------------------------------------------------------------- 6. Crash
const CMD = "kill -9 vibezeo";
export function Crash() {
  const f = useCurrentFrame();
  const typed = CMD.slice(0, Math.max(0, Math.min(CMD.length, Math.floor(f / 1.65))));
  const hit = 30;
  const g = f >= hit ? Math.min(1, (f - hit) / 22) : 0;
  const slices = f >= hit + 10 ? 6 : 0;
  const dead = f >= 55;
  const jitter = (k: string) => (f >= hit ? (random(`${k}${f}`) - 0.5) * 60 * g : 0);
  return (
    <AbsoluteFill>
      {!dead ? (
        <div style={{ position: "absolute", left: (1920 - 1360) / 2, top: 168, opacity: 1 - g * 0.3 }}>
          {[0, 1, 2].map((ch) =>
            ch === 0 || f >= hit ? (
              <div
                key={ch}
                style={{
                  position: ch === 0 ? "relative" : "absolute",
                  inset: ch === 0 ? undefined : 0,
                  transform: `translate(${ch === 1 ? -14 * g : ch === 2 ? 14 * g : jitter("j")}px, ${jitter("k") * 0.3}px)`,
                  mixBlendMode: ch === 0 ? undefined : "screen",
                  opacity: ch === 0 ? 1 : 0.55,
                  filter: ch === 1 ? "sepia(1) hue-rotate(-50deg) saturate(6)" : ch === 2 ? "sepia(1) hue-rotate(140deg) saturate(6)" : undefined,
                }}
              >
                <AppWindow src={APP} trimBefore={fr(CLIPS.crashBefore.from)} width={1360} aspect={APP_ASPECT} />
              </div>
            ) : null,
          )}
          {Array.from({ length: slices }, (_, k) => (
            <div
              key={k}
              style={{
                position: "absolute",
                left: jitter(`s${k}`) * 2,
                top: random(`t${k}${Math.floor(f / 2)}`) * 820,
                width: 1360,
                height: 10 + random(`h${k}${f}`) * 40,
                background: k % 2 ? "rgba(45,212,191,0.35)" : "rgba(6,10,18,0.9)",
                mixBlendMode: k % 2 ? "screen" : "normal",
              }}
            />
          ))}
        </div>
      ) : null}
      <AbsoluteFill style={{ background: "rgba(6,10,18,0.55)", opacity: f < hit ? 1 : 1 - g }} />
      <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", opacity: f < hit + 4 ? 1 : 0 }}>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 30 }}>
          <Words text="Now crash it." at={0} style={{ ...titleStyle, fontSize: 92 }} />
          <div
            style={{
              minWidth: 620,
              padding: "26px 34px",
              borderRadius: 18,
              background: "rgba(10,15,26,0.95)",
              border: `1px solid ${C.borderStrong}`,
              fontFamily: MONO,
              fontSize: 44,
              color: C.text,
              boxShadow: "0 30px 80px rgba(0,0,0,0.6)",
            }}
          >
            <span style={{ color: C.accent }}>$ </span>
            {typed}
            <span style={{ opacity: Math.floor(f / 6) % 2 ? 0 : 1, color: C.accent }}>▍</span>
          </div>
        </div>
      </AbsoluteFill>
      <AbsoluteFill style={{ background: "#000", opacity: dead ? 1 : 0 }} />
    </AbsoluteFill>
  );
}

// ---------------------------------------------------------------- 7. Restore
export function Restore({ zoom }: { zoom: Array<[number, number, number, number]> }) {
  const f = useCurrentFrame();
  const len = fr(10);
  const appear = pop(f, 0, 15, 140);
  return (
    <Scene len={len} inDur={4}>
      <div style={{ position: "absolute", top: 54, left: 0, right: 0, display: "flex", justifyContent: "center" }}>
        <Words text="Relaunch. It all comes back." at={6} accent={["comes", "back."]} style={{ ...titleStyle, fontSize: 58, justifyContent: "center" }} />
      </div>
      <div style={{ position: "absolute", left: (1920 - 1360) / 2, top: 168, transform: `scale(${0.92 + 0.08 * appear})`, opacity: Math.min(1, appear * 2) }}>
        <AppWindow src={APP} trimBefore={fr(CLIPS.restore.from)} rate={CLIPS.restore.rate} width={1360} aspect={APP_ASPECT} zoom={zoom} />
      </div>
      <Callout at={125} until={210} style={{ left: 90, bottom: 90 }}>
        Same layout, tabs and panes, in the same folders
      </Callout>
      <Callout at={215} until={290} style={{ left: 90, bottom: 90 }} color={C.amber}>
        Agents come back paused, ready to resume their session
      </Callout>
    </Scene>
  );
}

// ---------------------------------------------------------------- 8. Trust
function Icon({ kind }: { kind: "key" | "eye" | "seal" }) {
  const stroke = C.accent;
  const common = { fill: "none", stroke, strokeWidth: 1.8, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
  return (
    <svg width={56} height={56} viewBox="0 0 24 24">
      {kind === "key" ? (
        <>
          <circle cx="8" cy="15" r="4" {...common} />
          <path d="M11 12l8-8m-3 3l2 2m-4 0l2 2" {...common} />
        </>
      ) : kind === "eye" ? (
        <>
          <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z" {...common} />
          <circle cx="12" cy="12" r="3" {...common} />
          <path d="M3 3l18 18" {...common} />
        </>
      ) : (
        <>
          <path d="M12 2l3 2.2 3.6-.2 1 3.5 2.9 2.2-1.2 3.4 1.2 3.4-2.9 2.2-1 3.5-3.6-.2L12 22l-3-2.2-3.6.2-1-3.5L1.5 14.3 2.7 11 1.5 7.6l2.9-2.2 1-3.5 3.6.2z" {...common} />
          <path d="M8.5 12l2.5 2.5 4.5-5" {...common} />
        </>
      )}
    </svg>
  );
}

export function Trust() {
  const f = useCurrentFrame();
  const len = fr(6);
  const items = [
    { k: "key" as const, t: "Your accounts, untouched", b: "Each agent signs in by itself. Vibezeo never reads, copies or proxies an AI vendor's tokens." },
    { k: "eye" as const, t: "Private by default", b: "No analytics or crash reports unless you opt in. Secrets stay in your OS keychain." },
    { k: "seal" as const, t: "Signed updates", b: "Every update is signature-checked before it installs. Tampered builds are refused." },
  ];
  return (
    <Scene len={len}>
      <div style={{ position: "absolute", top: 170, left: 0, right: 0, display: "flex", flexDirection: "column", alignItems: "center", gap: 18 }}>
        <Eyebrow at={2}>Your machine</Eyebrow>
        <Words text="Your agents. Your accounts. Your machine." at={6} accent={["Your", "machine."]} style={{ ...titleStyle, fontSize: 68, justifyContent: "center" }} />
      </div>
      <div style={{ position: "absolute", top: 470, left: 0, right: 0, display: "flex", justifyContent: "center", gap: 34 }}>
        {items.map((it, i) => {
          const p = pop(f, 30 + i * 18, 14, 160);
          return (
            <div
              key={it.t}
              style={{
                width: 520,
                padding: "34px 34px 38px",
                borderRadius: 22,
                background: `linear-gradient(180deg, ${C.s3}, ${C.s2})`,
                border: `1px solid ${C.borderStrong}`,
                boxShadow: "0 40px 90px rgba(0,0,0,0.5)",
                transform: `translateY(${(1 - p) * 80}px) scale(${0.85 + 0.15 * p})`,
                opacity: Math.min(1, p * 1.5),
                display: "flex",
                flexDirection: "column",
                gap: 16,
              }}
            >
              <div style={{ width: 84, height: 84, borderRadius: 20, background: "rgba(45,212,191,0.10)", display: "flex", alignItems: "center", justifyContent: "center", border: "1px solid rgba(45,212,191,0.25)" }}>
                <Icon kind={it.k} />
              </div>
              <div style={{ fontFamily: FONT, fontWeight: 700, fontSize: 32, color: C.text, letterSpacing: "-0.02em" }}>{it.t}</div>
              <div style={{ ...subStyle, fontSize: 24 }}>{it.b}</div>
            </div>
          );
        })}
      </div>
    </Scene>
  );
}

// ---------------------------------------------------------------- 9. Claude
export function Claude() {
  const f = useCurrentFrame();
  const len = fr(4);
  const chips = ["Claude Code hooks drive live status", "--resume brings sessions back after a restart", "Most of Vibezeo was written with Claude Code"];
  return (
    <Scene len={len}>
      <AbsoluteFill style={{ alignItems: "center", justifyContent: "center" }}>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 18 }}>
          <Words text="Made for Claude Code." at={2} style={{ ...titleStyle, fontSize: 104, justifyContent: "center" }} />
          <Words text="Built with it." at={16} accent={["Built", "with", "it."]} style={{ ...titleStyle, fontSize: 104, justifyContent: "center" }} />
          <div style={{ display: "flex", gap: 18, marginTop: 40 }}>
            {chips.map((c, i) => {
              const p = pop(f, 30 + i * 6);
              return (
                <span
                  key={c}
                  style={{
                    padding: "14px 24px",
                    borderRadius: 999,
                    border: `1px solid ${C.borderStrong}`,
                    background: "rgba(13,19,32,0.9)",
                    fontFamily: c.startsWith("--") ? MONO : FONT,
                    fontSize: 25,
                    fontWeight: 600,
                    color: C.text,
                    transform: `translateY(${(1 - p) * 24}px)`,
                    opacity: Math.min(1, p * 1.5),
                  }}
                >
                  {c}
                </span>
              );
            })}
          </div>
        </div>
      </AbsoluteFill>
    </Scene>
  );
}

// ---------------------------------------------------------------- 10. End card
export function EndCard() {
  const f = useCurrentFrame();
  const len = fr(8.5);
  const flash = 1 - ramp(f, 0, 18);
  const out = ramp(f, len - 30, 30);
  return (
    <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", opacity: 1 - out }}>
      <AbsoluteFill style={{ background: "radial-gradient(circle at 50% 42%, rgba(94,234,212,0.45), transparent 50%)", opacity: flash }} />
      <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 22, transform: `scale(${1.04 - 0.04 * ramp(f, 0, 40)})` }}>
        <div style={{ display: "flex", alignItems: "center", gap: 28 }}>
          <Logo size={140} at={0} glow={1} />
          <span style={{ ...titleStyle, fontSize: 132, letterSpacing: "-0.05em", opacity: ramp(f, 6, 16), transform: `translateX(${(1 - ramp(f, 6, 20)) * -30}px)` }}>Vibezeo</span>
        </div>
        <Words text="A desktop workspace for the coding agents you already use." at={18} stagger={1.5} style={{ ...subStyle, fontSize: 36, justifyContent: "center" }} />
        <div style={{ display: "flex", alignItems: "center", gap: 22, marginTop: 34, opacity: ramp(f, 34, 16), transform: `translateY(${(1 - ramp(f, 34, 18)) * 20}px)` }}>
          <span
            style={{
              padding: "20px 36px",
              borderRadius: 14,
              background: C.accent,
              color: C.accentFg,
              fontFamily: FONT,
              fontWeight: 700,
              fontSize: 32,
              boxShadow: `0 0 ${30 + 20 * Math.sin(f / 10)}px rgba(45,212,191,0.45)`,
            }}
          >
            Download free for Linux
          </span>
          <span style={{ fontFamily: MONO, fontSize: 36, color: C.text }}>vibezeo.com</span>
        </div>
        <div style={{ fontFamily: FONT, fontSize: 22, color: C.subtle, marginTop: 6, opacity: ramp(f, 46, 16) }}>
          0.1.0 · Free plan · No account needed
        </div>
      </div>
    </AbsoluteFill>
  );
}

export const SCENES = { sec };
