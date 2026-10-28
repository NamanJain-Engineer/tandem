import { StatusDot, type Status } from "@tandem/ui/components/StatusDot";
import { GitBranch } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { cx } from "../../../lib/cx";
import { SCENE_BUILD, t } from "../../../lib/scenes";
import { Icon } from "../Icon";
import { engineTint } from "@tandem/ui/components/EngineGlyph";
import { engineMark } from "../home/Engines";

/** When something happens in a scene, in seconds from its start. Drawn as
 * `--at`, which every scene effect reads as its delay (scenes.css). */
export function at(seconds: number, extra?: Record<string, string | number>): CSSProperties {
  return { "--at": `${seconds}s`, ...extra } as CSSProperties;
}

/** The effects a scene element can have (scenes.css). The element's own
 * styles are its final frame: that's what reduced motion, a page without
 * the scene script and a finished scene all show. */
export type Fx = "in" | "line" | "pop" | "out" | "press" | "rise" | "slide" | "sweep" | "cut" | "show";

export function fx(kind: Fx, seconds: number, extra?: Record<string, string | number>) {
  return { [`data-fx-${kind}`]: "", style: at(seconds, extra) } as const;
}

/** Something that comes and goes: in at `seconds`, out `stay` seconds later.
 * Not in the final frame. */
export function visit(seconds: number, stay: number) {
  return { "data-fx-visit": "", style: at(seconds, { "--stay": `${stay}s` }) } as const;
}

/** An engine's monogram, as the app draws it (EngineGlyph, D-1895): the
 * registry's letters in fg on a 4 px tile in the engine's tint. */
export function Glyph({ engine, focused = false }: { engine: string; focused?: boolean }) {
  const mark = engineMark(engine);
  return (
    <span className={cx("sc-glyph", focused && "sc-glyph-on")} style={{ backgroundColor: engineTint(mark.id) }}>
      {mark.logo}
    </span>
  );
}

/** The app's status shapes (StatusDot), sized to the scene. */
export function Dot({ status }: { status: Status }) {
  return (
    <span className="sc-dot">
      <StatusDot status={status} size={10} />
    </span>
  );
}

/** A status that changes during the scene: `from` until `seconds`, then `to`.
 * The final frame is `to`. */
export function DotChange({ from, to, seconds }: { from: Status; to: Status; seconds: number }) {
  return (
    <span className="sc-dot-swap">
      <span {...fx("out", seconds)}>
        <Dot status={from} />
      </span>
      <span {...fx("pop", seconds)}>
        <Dot status={to} />
      </span>
    </span>
  );
}

/** The app's three modes, as the title bar's switch names them. */
export type Mode = "code" | "thread" | "agent";
const MODES: readonly Mode[] = ["code", "thread", "agent"];

/** The app's window: its title bar (the mark, the mode switch on `mode`) and
 * what's inside. Always the dark theme, as the app's captures are (D-1801). */
export function AppWindow({ children, className, mode = "code" }: { children: ReactNode; className?: string; mode?: Mode }) {
  return (
    <div data-theme="dark" className={cx("sc-window", className)}>
      <span className="sc-sheen" {...fx("sweep", 0.2)} />
      <div className="sc-titlebar">
        <span className="sc-brand">
          <svg viewBox="0 0 28 28" fill="none" className="sc-mark">
            <circle cx="10.5" cy="14" r="7" stroke="var(--tn-accent)" strokeWidth="2.2" />
            <circle cx="17.5" cy="14" r="7" stroke="var(--tn-accent)" strokeWidth="2.2" />
          </svg>
        </span>
        <span className="sc-modes">
          {MODES.map((m) => (
            <span key={m} className={cx("sc-mode", m === mode && "sc-mode-on")}>
              {t(`titlebar.mode.${m}`)}
            </span>
          ))}
        </span>
        <span className="sc-omnibox">{t("titlebar.omnibox")}</span>
      </div>
      {children}
    </div>
  );
}

/** The app's status bar as the download draws it (D-2603): the branch with
 * its icon, then the version. The early-access download has no hosted
 * service, so it shows no connection (D-816). */
export function StatusBar({ branch }: { branch: string }) {
  return (
    <div className="sc-statusbar">
      <span className="sc-statusbar-branch">
        <Icon icon={GitBranch} className="sc-statusbar-icon" />
        {branch}
      </span>
      <span className="sc-statusbar-right">
        <span>{t("status.version", { version: SCENE_BUILD })}</span>
      </span>
    </div>
  );
}

/** A pane: its header (glyph, its agent's first name and the title, branch,
 * status) over a terminal. */
export function Pane({
  engine,
  name,
  title,
  branch,
  status,
  focused = false,
  doneAt,
  children,
  className,
}: {
  engine?: string;
  /** The agent's first name (F-CD-12), shown before the title (PaneChrome). */
  name?: string;
  title: string;
  branch?: string;
  status: ReactNode;
  focused?: boolean;
  /** When the pane's agent finishes: its header lights once. */
  doneAt?: number;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cx("sc-pane", focused && "sc-pane-on", className)}>
      {doneAt !== undefined && <span className="sc-flash" {...visit(doneAt, 1.4)} />}
      <div className="sc-pane-head">
        {engine ? <Glyph engine={engine} focused={focused} /> : <span className="sc-glyph sc-glyph-shell">$</span>}
        {name && <span className="sc-pane-name">{name}</span>}
        <span className="sc-pane-title">{title}</span>
        {branch && <span className="sc-branch">{branch}</span>}
        <span className="sc-pane-status">{status}</span>
      </div>
      <div className="sc-term">{children}</div>
    </div>
  );
}

/** Terminal lines arriving one after another from `start`, `step` apart,
 * or spread evenly so the last arrives at `end`, however many there are
 * (a pane printing story.json's steps, before what comes next in it). */
export function Lines({ lines, start, step = 0.55, end }: { lines: readonly string[]; start: number; step?: number; end?: number }) {
  const gap = end !== undefined && lines.length > 1 ? (end - start) / (lines.length - 1) : step;
  return (
    <>
      {lines.map((l, i) => (
        <span key={l} className="sc-line" {...fx("line", Math.round((start + i * gap) * 100) / 100)}>
          {l}
        </span>
      ))}
    </>
  );
}

/** A number that counts up to `to` from `seconds` (a CSS counter, so the
 * final number is in the markup's style even without animation). */
export function Count({ to, seconds, prefix = "" }: { to: number; seconds: number; prefix?: string }) {
  return (
    <span className="sc-count" data-prefix={prefix} data-fx-count="" style={at(seconds, { "--to": to })}>
      <span className="sc-sr">{prefix + String(to)}</span>
    </span>
  );
}

/** A value that changes during the scene: each step from its time on, the
 * last one the final frame. */
export function Changes({ steps, className }: { steps: ReadonlyArray<readonly [number, ReactNode]>; className?: string }) {
  return (
    <span className={cx("sc-swap", className)}>
      {steps.map(([seconds, node], i) => {
        const next = steps[i + 1]?.[0];
        if (next === undefined && i === 0) return <span key={i}>{node}</span>;
        if (i === 0) return <span key={i} {...fx("out", next ?? 0)}>{node}</span>;
        if (next === undefined) return <span key={i} {...fx("pop", seconds)}>{node}</span>;
        return (
          <span key={i} {...visit(seconds, next - seconds)}>
            {node}
          </span>
        );
      })}
    </span>
  );
}
