import { StatusDot, type Status } from "@tandem/ui/components/StatusDot";
import { ChevronDown, GitBranch, Maximize2, MoreHorizontal, X, type LucideIcon } from "lucide-react";
import { Fragment, type CSSProperties, type ReactNode } from "react";
import { cx } from "../../../lib/cx";
import { SCENE_BUILD, t } from "../../../lib/scenes";
import { Icon } from "../Icon";
import { engineTint } from "@tandem/ui/components/EngineGlyph";
import { engineMark } from "../home/Engines";

/** `seconds` as CSS time, to the hundredth (a sum like 0.1 + 0.2 doesn't
 * print as 0.30000000000000004s). */
const secs = (seconds: number) => `${Math.round(seconds * 100) / 100}s`;

/** When something happens in a scene, in seconds from its start. Drawn as
 * `--at`, which every scene effect reads as its delay (scenes.css). */
export function at(seconds: number, extra?: Record<string, string | number>): CSSProperties {
  return { "--at": secs(seconds), ...extra } as CSSProperties;
}

/** The effects a scene element can have (scenes.css). The element's own
 * styles are its final frame: that's what reduced motion, a page without
 * the scene script and a finished scene all show. */
export type Fx = "in" | "line" | "enter" | "pop" | "out" | "press" | "rise" | "slide" | "sweep" | "cut" | "show" | "close";

export function fx(kind: Fx, seconds: number, extra?: Record<string, string | number>) {
  return { [`data-fx-${kind}`]: "", style: at(seconds, extra) } as const;
}

/** An element's effect, as `fx` and `visit` give it, passed on to a part. */
export type Motion = { style?: CSSProperties } & { [effect: `data-fx-${string}`]: string | undefined };

/** Something that comes and goes: in at `seconds`, then gone at once `stay`
 * seconds later, when the app removes it, since it animates no closing
 * content (D-3004). Not in the final frame. */
export function visit(seconds: number, stay: number) {
  return { "data-fx-visit": "", style: at(seconds, { "--stay": secs(stay) }) } as const;
}

/** A value's step between its first and its last (Changes): in as words
 * enter at `seconds`, cut at `until`, both on one element, so what it holds
 * sits where the other steps' does. Not in the final frame. */
export function stint(seconds: number, until: number) {
  return { "data-fx-stint": "", style: at(seconds, { "--until": secs(until) }) } as const;
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

/** An icon as the app draws it: lucide's, at its own stroke, sized by its
 * class, hidden from assistive tech (scenes are decorative) and without the
 * `xmlns` inline SVG doesn't need (the no-year check would read its 2000). */
export function AppIcon({ icon: Glyph, className }: { icon: LucideIcon; className?: string }) {
  return <Glyph size="1em" aria-hidden="true" focusable="false" xmlns={undefined} className={className} />;
}

/** The app's three modes, in the title bar's order (TitleBar's MODES). */
export type Mode = "agent" | "code" | "thread";
const MODES: readonly Mode[] = ["agent", "code", "thread"];

/** The app's window: its title bar (the mark, the mode switch on `mode` and,
 * in Code mode, the workspace menu's button, which names the workspace in
 * view) and what's inside. Always the dark theme, as the app's captures are
 * (D-1801). */
export function AppWindow({
  children,
  className,
  mode = "code",
  workspace,
}: {
  children: ReactNode;
  className?: string;
  mode?: Mode;
  workspace?: string;
}) {
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
        {mode === "code" && workspace && (
          <span className="sc-btn sc-btn-ghost sc-ws-button">
            {workspace}
            <AppIcon icon={ChevronDown} className="sc-ws-button-icon" />
          </span>
        )}
        <span className="sc-omnibox">{t("titlebar.omnibox")}</span>
      </div>
      {children}
    </div>
  );
}

/** The app's status bar as the download draws it (D-2603): the branch with
 * its icon, and what there is to push and pull once it has an upstream;
 * then a scene's segments (its CI, then its agents); the version at the
 * end. The early-access download has no hosted service, so it shows no
 * connection (D-816). */
export function StatusBar({ branch, counts, segments }: { branch: string; counts?: ReactNode; segments?: ReactNode }) {
  return (
    <div className="sc-statusbar">
      <span className="sc-statusbar-branch">
        <Icon icon={GitBranch} className="sc-statusbar-icon" />
        {branch}
        {counts}
      </span>
      {segments}
      <span className="sc-statusbar-right">
        <span>{t("status.version", { version: SCENE_BUILD })}</span>
      </span>
    </div>
  );
}

/** How many agents wait on you and how many are working, across every
 * agent, from a time on. */
export type Agents = { waiting: number; working: number };

/** The status bar's agents segment (StatusBar's AgentsSegment, after the
 * CI's): how many agents wait on you, in waiting's text, then how many are
 * working, each after its status dot; a count of none isn't drawn, and with
 * none of either the segment isn't (D-3004). `steps` are the counts from
 * each time on, the last the final frame (D-3002). */
export function AgentsSegment({ steps }: { steps: ReadonlyArray<readonly [number, Agents]> }) {
  const none = (n: Agents) => n.waiting === 0 && n.working === 0;
  if (steps.every(([, n]) => none(n))) return null;
  return (
    <Changes
      steps={steps.map(([seconds, n]) => [
        seconds,
        none(n) ? null : (
          <span key={seconds} className="sc-agents">
            {n.waiting > 0 && (
              <span className="sc-agents-waiting">
                <Dot status="waiting" />
                {t("status.waiting", { n: n.waiting })}
              </span>
            )}
            {n.working > 0 && (
              <span className="sc-agents-working">
                <Dot status="working" />
                {t("status.working", { n: n.working })}
              </span>
            )}
          </span>
        ),
      ])}
    />
  );
}

/** A pane: its header as PaneChrome draws it (UI_SPEC §2.20) over a
 * terminal. The header: its agent's status, its engine's glyph, the agent's
 * first name, the title, the worktree's chip (⎇ and its name, which the
 * site shows for its branch: D-1898), then at the far end how long its
 * agent has been at it while it works or waits, by the demo's clock
 * (Elapsed), and its buttons, quiet until the header is hovered: Maximize,
 * unless the pane is alone in its tab, then its menu and Close. The app's
 * grip shows only on hover. */
export function Pane({
  engine,
  name,
  title,
  branch,
  status,
  focused = false,
  solo = false,
  elapsed,
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
  /** Alone in its tab: no Maximize (F-CD-02), and no focus ring, which
   * only tells panes in a tab apart (PaneGrid); its glyph still lights. */
  solo?: boolean;
  /** How long its agent has been at it, while it works or waits
   * (PaneChrome's Elapsed), with when that shows. */
  elapsed?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cx("sc-pane", focused && !solo && "sc-pane-on", className)}>
      <div className="sc-pane-head">
        <span className="sc-pane-status">{status}</span>
        {engine ? <Glyph engine={engine} focused={focused} /> : <span className="sc-glyph sc-glyph-shell">$</span>}
        {name && <span className="sc-pane-name">{name}</span>}
        <span className="sc-pane-title">{title}</span>
        {branch && (
          <span className="sc-wt-chip">
            <AppIcon icon={GitBranch} className="sc-wt-chip-icon" />
            {branch}
          </span>
        )}
        {elapsed}
        <span className="sc-pane-tools">
          {!solo && <AppIcon icon={Maximize2} />}
          <AppIcon icon={MoreHorizontal} />
          <AppIcon icon={X} />
        </span>
      </div>
      <div className="sc-term">{children}</div>
    </div>
  );
}

/** Terminal lines arriving one after another from `start`, `step` apart,
 * or spread evenly so the last arrives at `end`, however many there are
 * (a pane printing story.json's steps, before what comes next in it). An
 * empty line is a little space, as a prompt's blank lines are; the lines
 * `fold` picks are left out on a phone, where a pane has less room. */
/** A line that's only this marks lines left out: an elision of the scene's,
 * drawn as one, not as something the terminal printed (DESIGN §4.6's
 * "simplified"; the design lead's condition on the CI pane's ⋮). */
export const ELISION = "⋮";

export function Lines({
  lines,
  start,
  step = 0.55,
  end,
  className,
  fold,
}: {
  lines: readonly string[];
  start: number;
  step?: number;
  end?: number;
  className?: string;
  fold?: (line: string, i: number) => boolean;
}) {
  const gap = end !== undefined && lines.length > 1 ? (end - start) / (lines.length - 1) : step;
  return (
    <>
      {lines.map((l, i) => {
        const narrow = fold?.(l, i) && "sc-hide-narrow";
        if (l === ELISION)
          return (
            <span key={i} className={cx("sc-elide", narrow)} {...fx("line", start + i * gap)}>
              {l}
            </span>
          );
        return l === "" ? (
          <span key={i} className={cx("sc-line-gap", narrow)} />
        ) : (
          <span key={i} className={cx("sc-line", className, narrow)} {...fx("line", start + i * gap)}>
            {l}
          </span>
        );
      })}
    </>
  );
}

/** A diff row as DiffView draws it: the mark, the code, and its old and new
 * line numbers (an added line has no old one, a removed one no new one). */
export type DiffRow = readonly [" " | "+" | "−", string, number | null, number | null];

/** A hunk's rows with notes under some of them (DiffView's annotation rows),
 * which come in together at `at`. From the first noted row down the rows
 * are drawn twice in one cell, as they were and with the notes: the two
 * swap at once, as the app inserts the notes in one go and the rows below
 * just move down, so no gap waits for a note and no rows are drawn twice;
 * only the notes fade in (D-3000). With no notes, just the rows. */
export function DiffRows({ rows, notes, at: arrive }: { rows: readonly DiffRow[]; notes: ReadonlyMap<number, ReactNode>; at: number }) {
  const first = notes.size > 0 ? Math.min(...notes.keys()) : rows.length - 1;
  const draw = (from: number, to: number, withNotes: boolean) =>
    rows.slice(from, to).map(([mark, code, oldNo, newNo], k) => (
      <Fragment key={from + k}>
        <span className={`sc-diff-line sc-diff-${mark === "+" ? "add" : mark === "−" ? "del" : "ctx"}`}>
          <span className="sc-diff-no">{oldNo ?? ""}</span>
          <span className="sc-diff-no">{newNo ?? ""}</span>
          <span className="sc-diff-mark">{mark}</span>
          <span className="sc-diff-code">{code}</span>
        </span>
        {withNotes && notes.get(from + k)}
      </Fragment>
    ));
  return (
    <div className="sc-diff-body">
      {draw(0, first + 1, false)}
      {notes.size > 0 && (
        <span className="sc-diff-swap">
          <span className="sc-diff-rows" {...fx("cut", arrive)}>
            {draw(first + 1, rows.length, false)}
          </span>
          <span className="sc-diff-rows" {...fx("show", arrive)}>
            {notes.get(first)}
            {draw(first + 1, rows.length, true)}
          </span>
        </span>
      )}
    </div>
  );
}

/** A toast's tone, which its bar shows (Toast.tsx's ToastTone). */
export type Tone = "info" | "success" | "warning" | "error";

/** A toast as the app draws it (Toast.tsx, UI_SPEC §2.12): 360 px at the
 * bottom right, a 3 px bar in its tone, its title, its body and its action
 * (a small secondary button) under them, and × to dismiss it. It comes and
 * goes: in at `at`, gone `stay` seconds later. Its action pressed at
 * `press` dismisses it, as the app's does. */
export function Toast({
  tone = "info",
  title,
  body,
  action,
  at: shown,
  stay = 1.5,
  press,
}: {
  tone?: Tone;
  title: string;
  body?: string;
  action?: string;
  at: number;
  stay?: number;
  press?: number;
}) {
  return (
    <div className={`sc-note-toast sc-tone-${tone}`} {...visit(shown, stay)}>
      <span className="sc-note-toast-text">
        <span className="sc-note-toast-title">{title}</span>
        {body && <span className="sc-note-toast-body">{body}</span>}
        {action && (
          <span className="sc-note-toast-action">
            <span className="sc-btn sc-btn-sm" {...(press === undefined ? {} : fx("press", press))}>
              {action}
            </span>
          </span>
        )}
      </span>
      <AppIcon icon={X} className="sc-note-toast-x" />
    </div>
  );
}

/** The app's Badge variants the scenes draw (Badge.tsx). */
export type BadgeVariant = "neutral" | "waiting" | "done" | "error" | "outline";

/** The app's Badge (Badge.tsx, UI_SPEC §2.13): 18 px, 10 px medium text in
 * its tone's text colour, a border in its tone and no fill (neutral is the
 * one with a fill, and no border); `quiet`, no box: its 11 px text alone,
 * for a row that must stay calm (D-2601). */
export function Badge({ variant = "neutral", quiet = false, children }: { variant?: BadgeVariant; quiet?: boolean; children: ReactNode }) {
  return <span className={cx(quiet ? "sc-badge-quiet" : "sc-badge", `sc-badge-${variant}`)}>{children}</span>;
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
 * last one the final frame. The app re-renders a value at once, so a step
 * is gone the moment the next comes, and the next enters as words do
 * (DESIGN §4.7: a fade and a short rise). Two steps are never drawn
 * together, mid-swap either (D-3002). Each step is one element with what
 * it holds directly inside, first, last or between (a stint), so a rule
 * for a step's contents finds them at one depth in every step. */
export function Changes({ steps, className }: { steps: ReadonlyArray<readonly [number, ReactNode]>; className?: string }) {
  return (
    <span className={cx("sc-swap", className)}>
      {steps.map(([seconds, node], i) => {
        const next = steps[i + 1]?.[0];
        const effect = i === 0 ? (next === undefined ? {} : fx("cut", next)) : next === undefined ? fx("enter", seconds) : stint(seconds, next);
        return (
          <span key={i} {...effect}>
            {node}
          </span>
        );
      })}
    </span>
  );
}

/** The review queue's buttons for a worktree's work, in ReviewCard's order:
 * Approve the primary, Open diff secondary, the rest ghosts, Run check
 * among them once the repository's check isn't running. Second opinion is
 * pressed at `opinion`, when it is. */
function ReviewActions({ ranCheck, opinion }: { ranCheck: boolean; opinion?: number }) {
  const ghost = (key: string, press?: number) => (
    <span className="sc-btn sc-btn-sm sc-btn-ghost" {...(press === undefined ? {} : fx("press", press))}>
      {t(key)}
    </span>
  );
  return (
    <span className="sc-rq-actions">
      <span className="sc-btn sc-btn-sm sc-btn-primary">{t("review.approve")}</span>
      <span className="sc-btn sc-btn-sm">{t("review.openDiff")}</span>
      {ghost("review.sendBack")}
      {ghost("review.secondOpinion", opinion)}
      {ghost("review.merge")}
      {ranCheck && ghost("review.runCheck")}
      {ghost("review.snooze")}
      {ghost("review.jump")}
      {ghost("review.repo.menu")}
    </span>
  );
}

/** A card in the review queue as ReviewQueue's ReviewCard draws it
 * (F-DB-04): its title, the agent's first name; its engine, its worktree's
 * name (its workspace and its branch, tandem/<name>, shown once: D-1898)
 * and how long ago it finished; the agent's closing words; the diff's size;
 * its risks, each an outlined Badge; the repository's check, running until
 * `passed` (passed throughout without it); then its buttons. The diff's
 * numbers count up from `count` when it's given, a scene's effect: the app
 * writes them at once. */
export function ReviewCard({
  lane,
  minutes,
  closing,
  diff,
  risks,
  passed,
  count,
  opinion,
}: {
  lane: { name: string; engine: string; branch: string };
  minutes: number;
  closing: string;
  diff: { files: number; adds: number; dels: number };
  risks: readonly string[];
  passed?: number;
  count?: number;
  opinion?: number;
}) {
  const stat = { n: diff.files, adds: diff.adds, dels: diff.dels };
  return (
    <span className="sc-rq-card">
      <span className="sc-rq-title">{lane.name}</span>
      <span className="sc-rq-meta">{[lane.engine, lane.branch, t("time.minutes", { n: minutes })].join(" · ")}</span>
      <span className="sc-rq-closing">{closing}</span>
      <span className="sc-rq-stat">
        {count === undefined ? (
          t("review.stat", stat)
        ) : (
          <>
            {t("review.stat", { ...stat, adds: 0, dels: 0 }).split(" · ")[0]} · <Count to={diff.adds} seconds={count} prefix="+" />{" "}
            <Count to={diff.dels} seconds={count} prefix="−" />
          </>
        )}
      </span>
      {risks.length > 0 && (
        <span className="sc-rq-risks">
          {risks.map((r) => (
            <Badge key={r} variant="outline">
              {r}
            </Badge>
          ))}
        </span>
      )}
      <span className="sc-rq-check">
        {passed === undefined ? (
          t("review.check.passed")
        ) : (
          <Changes
            steps={[
              [0, t("review.check.running")],
              [passed, t("review.check.passed")],
            ]}
          />
        )}
      </span>
      {passed === undefined ? (
        <ReviewActions ranCheck opinion={opinion} />
      ) : (
        <span className="sc-rq-actions-swap">
          <span {...fx("cut", passed)}>
            <ReviewActions ranCheck={false} opinion={opinion} />
          </span>
          <span {...fx("show", passed)}>
            <ReviewActions ranCheck opinion={opinion} />
          </span>
        </span>
      )}
    </span>
  );
}

/** A message typed into an agent as one paste, as its terminal echoes it:
 * the first line after the prompt mark, the rest under it, its blank lines
 * left blank. */
export function pasted(text: string): string[] {
  let first = true;
  return text.split("\n").map((l) => {
    if (l === "") return "";
    const line = `${first ? "› " : "  "}${l}`;
    first = false;
    return line;
  });
}

/** The review queue as ReviewQueue draws it with work in it: its sort, by
 * risk, and Refresh; then To review with its count, over its cards. */
export function ReviewQueue({ count, children, ...motion }: { count: number; children: ReactNode } & Motion) {
  return (
    <span className="sc-rq" {...motion}>
      <span className="sc-rq-sort">
        <span className="sc-seg sc-seg-sm">
          <span className="sc-mode sc-mode-on">{t("review.byRisk")}</span>
          <span className="sc-mode">{t("review.byTime")}</span>
        </span>
        <span className="sc-btn sc-btn-sm sc-btn-ghost">{t("review.refresh")}</span>
      </span>
      <span className="sc-rq-section">{`${t("review.toReview")} · ${count}`}</span>
      {children}
    </span>
  );
}
