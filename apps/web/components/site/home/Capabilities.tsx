import { EngineGlyph } from "@tandem/ui/components/EngineGlyph";
import { StatusDot } from "@tandem/ui/components/StatusDot";
import {
  ArrowRight,
  Check,
  GitBranch,
  Lock,
  MessageSquareText,
  Play,
  Plug,
  Search,
  Server,
} from "lucide-react";
import type { ReactElement } from "react";
import { CAPABILITIES, MORE } from "../../../lib/home";
import { Icon } from "../Icon";
import { iconFor } from "../icons";
import { Keys } from "../Keys";
import { LitGlow } from "../LitGlow";
import { Section } from "../Section";
import { stagger } from "../../../lib/motion";
import { engineMark } from "./Engines";
import "./capabilities.css";

type Title = (typeof CAPABILITIES)[number]["title"];

/** A small engine tile, as the app draws it beside a pane's name. */
function Tile({ engine }: { engine: string }) {
  const m = engineMark(engine);
  return <EngineGlyph letters={m.logo} engine={m.id} size={16} active />;
}

/* Each tile's picture: a fragment of the real UI, drawn with the app's own
   tokens and components, showing what the capability does in this build
   (the docs say the rest). Decorative: the tile's name is its label. */

/** Find work (code-mode docs): describe it in the palette, and it lists the
 * panes, threads and teammates doing it, best first. */
function FindWorkArt() {
  return (
    <div className="cap-palette">
      <div className="cap-input">
        <Icon icon={Search} size={13} />
        <span className="cap-typed">the login tests</span>
        <span className="cap-caret" />
        <Keys id="palette.open" className="ml-auto" />
      </div>
      <div className="cap-result is-active">
        <StatusDot status="working" size={8} />
        <span className="cap-result-title">Fix flaky auth.spec.ts</span>
        <span className="cap-meta">pane · fix-login</span>
      </div>
      <div className="cap-result">
        <Icon icon={MessageSquareText} size={12} />
        <span className="cap-result-title">Why does login time out?</span>
        <span className="cap-meta">thread</span>
      </div>
    </div>
  );
}

/** Dictate: hold Ctrl+Alt, speak, let go. */
function DictateArt() {
  const bars = [5, 9, 14, 20, 12, 17, 8, 13, 6];
  return (
    <div className="cap-dictate">
      <span className="cap-chord">
        <kbd className="cap-key">Ctrl</kbd>
        <kbd className="cap-key">Alt</kbd>
      </span>
      <span className="cap-wave">
        {bars.map((h, i) => (
          <span key={i} style={{ height: `${h}px` }} />
        ))}
      </span>
    </div>
  );
}

/** SSH workspaces: a folder on another machine, its host key pinned. */
function SshArt() {
  return (
    <div className="cap-stack">
      <div className="cap-row">
        <Icon icon={Server} size={12} />
        <span className="cap-strong">build-box</span>
        <span className="cap-meta ml-auto">arm64</span>
      </div>
      <div className="cap-row cap-row-quiet">
        <Icon icon={Lock} size={12} />
        <span>host key pinned</span>
      </div>
    </div>
  );
}

/** Worktree lanes: each task on its own worktree and branch, merged back. */
function WorktreeArt() {
  return (
    <div className="cap-lanes">
      <div className="cap-lane-labels">
        <span className="cap-lane-label">
          <Tile engine="Claude Code" />
          fix-login
        </span>
        <span className="cap-lane-label">
          <Tile engine="Codex" />
          coupon-rules
        </span>
        <span className="cap-lane-label cap-lane-main">
          <Icon icon={GitBranch} size={12} />
          main
        </span>
      </div>
      <svg className="cap-lane-graph" viewBox="0 0 220 84" preserveAspectRatio="xMinYMid meet" fill="none">
        <path className="cap-lane-line" d="M0 70H220" />
        <path className="cap-lane-line is-lit" d="M0 14H132C160 14 160 70 188 70" />
        <path className="cap-lane-line" d="M0 42H116" strokeDasharray="3 4" />
        {[28, 72, 116].map((x) => (
          <circle key={`a${x}`} className="cap-node is-lit" cx={x} cy="14" r="3.5" />
        ))}
        {[28, 72].map((x) => (
          <circle key={`b${x}`} className="cap-node" cx={x} cy="42" r="3.5" />
        ))}
        <circle className="cap-node" cx="12" cy="70" r="3.5" />
        <circle className="cap-node is-merge" cx="188" cy="70" r="4.5" />
      </svg>
    </div>
  );
}

/** Previews per worktree: each worktree's app on its own port, side by side. */
function PreviewsArt() {
  const sites = [
    { worktree: "fix-login", port: 41010, variant: "a" },
    { worktree: "coupon-rules", port: 41020, variant: "b" },
  ];
  return (
    <div className="cap-previews">
      {sites.map((s) => (
        <div key={s.worktree} className="cap-browser" data-variant={s.variant}>
          <div className="cap-browser-bar">
            <span className="cap-url">{s.worktree}</span>
            <span className="cap-port">:{s.port}</span>
          </div>
          <div className="cap-browser-page">
            <span className="cap-sk cap-sk-title" />
            <span className="cap-sk" />
            <span className="cap-sk cap-sk-short" />
            <span className="cap-sk cap-sk-button" />
          </div>
        </div>
      ))}
    </div>
  );
}

/** Crash-safe restore: panes come back, agents paused, ready to resume. */
function RestoreArt() {
  return (
    <div className="cap-stack">
      {["Claude Code", "Codex"].map((e) => (
        <div key={e} className="cap-row cap-pane">
          <Tile engine={e} />
          <StatusDot status="idle" size={8} />
          <span>Paused</span>
          <Icon icon={Play} size={11} className="ml-auto" />
        </div>
      ))}
    </div>
  );
}

/** Plugins with approvals: reads run, writes ask you first. */
function PluginsArt() {
  return (
    <div className="cap-stack">
      <div className="cap-row cap-row-quiet">
        <Icon icon={Plug} size={12} />
        <span>Read issues</span>
        <span className="cap-meta ml-auto inline-flex items-center gap-1">
          <Icon icon={Check} size={11} />
          ran
        </span>
      </div>
      <div className="cap-ask">
        <div className="cap-row">
          <StatusDot status="waiting" size={8} />
          <span className="cap-strong">Create an issue?</span>
          <span className="cap-meta">write · asks first</span>
        </div>
        <div className="cap-ask-actions">
          <span className="cap-btn">Deny</span>
          <span className="cap-btn cap-btn-strong">Allow once</span>
        </div>
      </div>
    </div>
  );
}

/** Files, git and a browser: the editor's diff of what an agent changed. */
function FilesArt() {
  return (
    <div className="cap-diff">
      <span className="cap-diff-file">session.ts</span>
      <span className="cap-diff-line is-del">- if (!user)</span>
      <span className="cap-diff-line is-add">+ if (!session)</span>
      <span className="cap-diff-line">{"  "}return null</span>
    </div>
  );
}

/** The bento, in reading order: wide tiles take two of six columns. */
const BENTO: ReadonlyArray<{ title: Title; wide?: boolean; Art: () => ReactElement }> = [
  { title: "Find work by what it's about", wide: true, Art: FindWorkArt },
  { title: "Dictate", Art: DictateArt },
  { title: "SSH workspaces", Art: SshArt },
  { title: "Worktree lanes and handoff", wide: true, Art: WorktreeArt },
  { title: "Previews per worktree", wide: true, Art: PreviewsArt },
  { title: "Crash-safe restore", Art: RestoreArt },
  { title: "Plugins with approvals", wide: true, Art: PluginsArt },
  { title: "Files, git and a browser", Art: FilesArt },
];

/** #capabilities (§6.9, D-1881, D-1885): the eight capabilities people use
 * most as a bento of tiles, each its name (linking to its docs) over a small
 * picture made of the real UI; then everything else as a line of links. What
 * each does is in the docs. The tiles catch the pointer's light. */
export function Capabilities() {
  const tiles = CAPABILITIES.map((c) => {
    const b = BENTO.find((x) => x.title === c.title);
    if (!b) throw new Error(`${c.title} has no tile in the capability bento`);
    return { ...c, ...b, order: BENTO.indexOf(b) };
  }).sort((a, b) => a.order - b.order);
  return (
    <Section id="capabilities" eyebrow="Everything in the app" title="Built in, working today" divider>
      <ul className="cap-bento">
        {tiles.map(({ title, docs, icon, wide, Art }, i) => (
          <li key={title} className={`reveal ${stagger(i)} ${wide ? "cap-wide" : ""}`}>
            <a href={docs} data-lit="" className="cap-tile group">
              <LitGlow />
              <div aria-hidden="true" className="cap-art">
                <Art />
              </div>
              <span className="cap-name">
                <Icon icon={iconFor(icon)} size={16} className="shrink-0 text-fg-subtle" />
                <span>{title}</span>
                <Icon icon={ArrowRight} size={14} className="cap-arrow" />
              </span>
            </a>
          </li>
        ))}
      </ul>
      <p className="reveal flex flex-wrap items-baseline gap-x-2 gap-y-1 text-copy-sm text-fg-muted">
        <a
          href="/docs/"
          className="font-medium text-fg underline decoration-[color-mix(in_oklab,currentColor_35%,transparent)] underline-offset-[0.2em]"
        >
          Everything else in the docs
        </a>
        <span aria-hidden="true">·</span>
        {MORE.map((m, i) => (
          <span key={m.title}>
            <a
              href={m.docs}
              className="text-fg-muted underline decoration-[color-mix(in_oklab,currentColor_35%,transparent)] underline-offset-[0.2em] hover:text-fg"
            >
              {m.title}
            </a>
            {i < MORE.length - 1 ? "," : "."}
          </span>
        ))}
      </p>
    </Section>
  );
}
