// The demo's prompts are the core's, word for word (D-1899: one world from
// one source). The core writes them in Rust, so these tests read its
// sources: a change there that the demo doesn't follow fails here, as
// D-3022's `@` did (F-WT-08, F-WT-10).
import { describe, expect, it } from "vitest";
import UNTRUSTED from "../../../../crates/tandem-core/src/untrusted.rs?raw";
import PR_CHECKS from "../../src-tauri/src/services/pr_checks.rs?raw";
import RADAR from "../../src-tauri/src/services/radar.rs?raw";
import { DemoCore } from "./backend";
import { DEFANGED, commentFenced, defangMentions, failurePrompt, fence, ghLog, untrustedLabel } from "./ci";
import { STORY, worktree } from "./story";

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
  it("F-WT-08 the untrusted label, the fence and the note after a defanged @ are untrusted.rs's", () => {
    expect(untrustedLabel("the CI check")).toBe(
      fill(literal(UNTRUSTED, "> Untrusted content from"), { source: "the CI check" }),
    );
    expect(DEFANGED).toBe(literal(UNTRUSTED, "Vibezeo wrote"));
    // untrusted.rs's own cases: a path after an @ is defanged, a hunk
    // header's @@ and an @ before a space aren't.
    expect(defangMentions("npm ERR! see @/home/u/.npmrc\n@@ -1,2 +1,3 @@")).toBe(
      "npm ERR! see ＠/home/u/.npmrc\n@@ -1,2 +1,3 @@",
    );
    expect(defangMentions("a @ b @")).toBe("a @ b @");
    expect(fence("text", "a ``` b")).toBe("````text\na ``` b\n````");
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
    const fenced = fence("text", defangMentions(ci));
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
      `${untrustedLabel("a pull request review comment")}\n${fence("text", defangMentions(text))}`,
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
