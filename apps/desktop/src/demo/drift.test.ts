// The demo's prompts are the core's, word for word (D-1899: one world from
// one source). The core writes them in Rust, so these tests read its
// sources: a change there that the demo doesn't follow fails here, as
// D-3022's `@` did (F-WT-08, F-WT-10).
import { describe, expect, it } from "vitest";
import HEURISTICS from "../../../../crates/tandem-core/src/status_heuristics.rs?raw";
import REPORT from "../../../../crates/tandem-core/src/status_report.rs?raw";
import UNTRUSTED from "../../../../crates/tandem-core/src/untrusted.rs?raw";
import HOOK from "../../../../crates/tandem-engines/src/hook.rs?raw";
import REGISTRY from "../../../../crates/tandem-engines/registry.json";
import VECTORS from "../../../../crates/tandem-core/tests/fixtures/defang.json";
import DASHBOARD from "../../src-tauri/src/services/dashboard.rs?raw";
import PR_CHECKS from "../../src-tauri/src/services/pr_checks.rs?raw";
import RADAR from "../../src-tauri/src/services/radar.rs?raw";
import { DEFANGED, defang } from "../features/code/untrusted";
import { DemoCore } from "./backend";
import { commentFenced, failurePrompt, fence, fenceDefanged, ghLog, untrustedLabel } from "./ci";
import { STORY, askLine, previewOf, worktree, worktreeIn, type StoryWorktree } from "./story";

/** A Rust string literal's text: a `\` at a line's end joins the next line
 * without its indent, and `\n`, `\t`, `\"` and `\\` are what they say. */
function rust(body: string): string {
  return body
    .replace(/\\\n\s*/g, "")
    .replace(/\\(.)/g, (_, c: string) => (c === "n" ? "\n" : c === "t" ? "\t" : c));
}

/** The string literal in `source` that starts with `start`. */
function literal(source: string, start: string): string {
  const at = source.indexOf(`"${start}`);
  expect(at, start).toBeGreaterThanOrEqual(0);
  const m = /^"((?:[^"\\]|\\[\s\S])*)"/.exec(source.slice(at));
  return rust(m?.[1] ?? "");
}

/** `format!`'s placeholders filled in order: named ones by name, `{}` by position. */
function fill(template: string, named: Record<string, string>, positional: string[] = []): string {
  let i = 0;
  return template.replace(/\{(\w*)\}/g, (_, name: string) => (name ? (named[name] ?? `{${name}}`) : (positional[i++] ?? "{}")));
}

describe("F-WT-08 F-WT-10 D-3022 the demo's prompts are the core's (D-1899)", () => {
  it("F-WT-08 D-3023 the untrusted label, the fence and fence_defanged are untrusted.rs's, on every shared vector", () => {
    expect(untrustedLabel("the CI check")).toBe(
      fill(literal(UNTRUSTED, "> Untrusted content from"), { source: "the CI check" }),
    );
    expect(DEFANGED).toBe(literal(UNTRUSTED, "Vibezeo wrote"));
    expect(fence("text", "a ``` b")).toBe("````text\na ``` b\n````");
    // fence_defanged: the text defang makes, fenced, then DEFANGED on its
    // own line when defang wrote ＠.
    expect(UNTRUSTED).toMatch(
      /pub fn fence_defanged\(info: &str, text: &str\) -> String \{\s*let \(safe, wrote\) = defang_marked\(text\);\s*let fenced = fence\(info, &safe\);\s*if wrote \{\s*format!\("\{fenced\}\\n\{DEFANGED\}"\)\s*\} else \{\s*fenced\s*\}/,
    );
    // The vectors both suites read: removable characters, @ runs, /skill
    // words, paste markers and ultra words (D-3023 to D-3034).
    const { note, cases } = VECTORS as { note: string; cases: { in: string; out: string }[] };
    expect(note).toBe(DEFANGED);
    const ats = (text: string) => [...text].filter((c) => c === "\uFF20").length;
    for (const c of cases) {
      const fenced = fence("text", c.out);
      expect(fenceDefanged("text", c.in), JSON.stringify(c.in)).toBe(
        ats(c.out) > ats(c.in) ? `${fenced}\n${note}` : fenced,
      );
    }
  });

  it("F-WT-08.1 Send failure to agent's prompt is pr_checks.rs's failure_prompt, the log's @ defanged", () => {
    const check = STORY.pr.checks.find((c) => c.bucket === "fail");
    if (!check) throw new Error("story.json has no failing check");
    const prompt = failurePrompt(STORY.pr.number, check);
    // The failed step's log as gh prints it (pr_checks.rs keeps its lines).
    const log = ghLog(check);
    expect(log.split("\n")[0]).toMatch(/^test\tRun pnpm test\t\S+Z > shop@1\.4\.0 test /);
    const ci = [
      fill(literal(PR_CHECKS, "Check: {}"), {}, [check.name]),
      fill(literal(PR_CHECKS, "\\nWorkflow: {w}"), { w: check.workflow }),
      fill(literal(PR_CHECKS, "\\nLink: {l}"), { l: check.link }),
      fill(literal(PR_CHECKS, "\\n\\nFailed steps' log (last lines):\\n{log}"), { log }),
    ].join("");
    const fenced = fence("text", defang(ci));
    expect(prompt).toBe(
      fill(literal(PR_CHECKS, "A CI check failed"), { pr: String(STORY.pr.number) }, [
        untrustedLabel("the CI check"),
        `${fenced}\n${DEFANGED}`,
      ]),
    );
    expect(PR_CHECKS).toContain('untrusted::label("the CI check")');
    expect(PR_CHECKS).toContain('untrusted::fence_defanged("text", &ci)');
  });

  it("F-WT-08.2 an imported comment is comment_fenced's: its author with no @, behind the label", () => {
    const c = STORY.pr.comments[0];
    if (!c) throw new Error("story.json has no review comment");
    const place = fill(literal(PR_CHECKS, "{path}:{line}"), { path: c.file, line: String(c.line) });
    const text = fill(literal(PR_CHECKS, "{author} on {place}:\\n{}"), { author: c.author, place }, [c.body.trim()]);
    expect(commentFenced(c.author, c.file, c.line, c.body)).toBe(
      `${untrustedLabel("a pull request review comment")}\n${fence("text", defang(text))}`,
    );
    expect(PR_CHECKS).toContain('untrusted::label("a pull request review comment")');
  });

  it("F-WT-10 Warn both agents types radar.rs's note_text", () => {
    const core = new DemoCore(0);
    const [a, b] = STORY.overlap.between;
    if (!a || !b) throw new Error("story.json's overlap names no pair");
    const lane = (slug: string) => `ws_${slug.replace(/-/g, "_")}`;
    core.handle("lane_overlap", { laneId: lane(worktree(a).slug) });
    const note = core.handle("lane_radar_note", { laneId: lane(a), otherId: lane(b) }) as string;
    const [head = "", rest = ""] = literal(RADAR, "Heads-up from Vibezeo").split("\n\n{}\n{}");
    expect(rest).toBe("");
    expect(note.startsWith(`${head}\n\n${untrustedLabel("the overlapping symbols")}\n`)).toBe(true);
    expect(RADAR).toContain('untrusted::label("the overlapping symbols")');
    expect(RADAR).toContain('untrusted::fence_defanged("text", data.trim_end())');
    // The data: the other worktree, then a line a symbol, as note_text writes them.
    const body = note.slice(note.indexOf("```text\n") + 8, note.lastIndexOf("\n```"));
    // Each of note_text's lines ends with a newline; the last one is trimmed.
    const other = fill(literal(RADAR, "Other worktree: {other_branch}"), { other_branch: `tandem/${b}` });
    expect(`${body.split("\n")[0]}\n`).toBe(other);
    const line = literal(RADAR, "- {}: {} here, {} there").replace(/\n$/, "");
    for (const row of body.split("\n").slice(1)) expect(row).toMatch(new RegExp(`^${line.replace(/[.*+?^$()|[\]\\]/g, "\\$&").replace(/\{\}/g, ".+")}$`));
  });
});

/** An engine's entry in the app's registry. */
const engine = (id: string) =>
  (REGISTRY as Array<{ id: string; waitingPatterns: string[]; hooksSupport: string }>).find(
    (e) => e.id === id,
  );

/** The body of the first `Some(StatusUpdate { … })` in a Rust source. */
function statusUpdate(source: string): string {
  const at = source.indexOf("Some(StatusUpdate {");
  expect(at).toBeGreaterThanOrEqual(0);
  return source.slice(at, source.indexOf("})", at));
}

describe("F-DB-01 F-SH-03 D-2624 the waiting agent and the Dashboard's cards are what the app shows (D-1899)", () => {
  it("the waiting agent asks in its engine's words, a line its registry waitingPatterns match", () => {
    const w = worktreeIn("waiting");
    const patterns = engine(w.engine)?.waitingPatterns ?? [];
    expect(patterns.length).toBeGreaterThan(0);
    // terminal.rs watches an agent pane's screen with these: a question
    // they don't match leaves the agent working, never waiting.
    expect(patterns.some((p) => new RegExp(p).test(askLine(w)))).toBe(true);
    expect(askLine(w)).toBe(`Allow execution of: '${w.ask}'?`);
    // 0.12.0's demo line matched none of them.
    expect(patterns.some((p) => new RegExp(p).test(`Allow running ${w.ask}? (y/n)`))).toBe(false);
  });

  it("each story engine reports what its registry entry says it can", () => {
    for (const [id, e] of Object.entries(STORY.engines))
      expect(e.hooksSupport, id).toBe(engine(id)?.hooksSupport);
  });

  it("a pane's card line is its status record's preview, else its tool; only Claude Code's hooks name a tool, while it works", () => {
    // dashboard.rs draws the card's line from the agent's status record.
    expect(DASHBOARD).toContain("preview: redact(rec.preview.or(rec.tool))");
    // Without hooks, the status comes from the screen: no tool, no preview.
    expect(statusUpdate(HEURISTICS)).toMatch(/tool: None,[\s\S]*preview: None,/);
    // A hook's report carries no preview, and the tool its hook named.
    expect(statusUpdate(REPORT)).toMatch(/tool: self\.payload\.tool\.clone\(\),[\s\S]*preview: None,/);
    // Claude Code's PreToolUse names its tool; its Stop and Codex's
    // turn-complete name none.
    expect(HOOK).toMatch(/"PreToolUse" => \{\s*payload\.tool = field\("tool_name"\)/);
    expect(HOOK).toContain(`"Stop" => ReportEvent::Done,`);
    expect(HOOK).toContain(`"agent-turn-complete" => ReportEvent::Done,`);
    // So the opening frame's cards have no line: Ada has finished, Arlo's
    // Codex names no tools, and Bea's Gemini CLI has no hooks.
    for (const w of STORY.worktrees) expect(previewOf(w), w.slug).toBeNull();
    // A Claude Code agent at work names the tool it's running.
    const at = (w: StoryWorktree, steps: StoryWorktree["steps"]): StoryWorktree => ({
      ...w,
      state: "working",
      steps,
    });
    expect(previewOf(at(worktreeIn("done"), [{ read: "src/a.ts" }]))).toBe("Read");
    expect(previewOf(at(worktreeIn("done"), [{ edit: "src/a.ts", add: 1, del: 1 }]))).toBe("Edit");
    expect(previewOf(at(worktreeIn("done"), [{ run: "pnpm test" }]))).toBe("Bash");
    expect(previewOf(at(worktreeIn("waiting"), [{ run: "pnpm test" }]))).toBeNull();
    expect(previewOf(at(worktreeIn("working"), [{ run: "pnpm test" }]))).toBeNull();
  });
});
