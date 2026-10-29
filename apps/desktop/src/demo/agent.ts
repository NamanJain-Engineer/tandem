import type { FileDiff, ThreadEvent } from "@tandem/protocol";

type ToolEvent = Extract<ThreadEvent, { type: "tool" }>;
import { MAIN, changedIn, diffsOf, grep, lineCount, testRun } from "./repo";

/** One step of a simulated turn: an event after a pause, or a stop that
 * waits for the visitor to answer an approval. */
export type Step = { wait: number; event: ThreadEvent } | { approval: ThreadEvent & { type: "approval_request" } };

const tool = (
  id: string,
  verb: ToolEvent["verb"],
  argSummary: string,
  status: ToolEvent["status"],
  extra: Partial<ToolEvent> = {},
): ThreadEvent => ({
  type: "tool",
  id,
  verb,
  argSummary,
  status,
  output: null,
  outputTruncated: false,
  outputFile: null,
  diff: null,
  detail: null,
  ...extra,
});

/** The first thread's fix, in `shop` itself on main (`shop/thread/`):
 * `getSession` renews a session that expired a moment ago, and its tests
 * say so. */
export const THREAD_CHANGES: FileDiff[] = diffsOf("thread");

const SESSION = "src/auth/session.ts";
const SESSION_TEST = "src/auth/session.test.ts";

/** A Claude Code Edit as its stream carries it (D-2608): no diff, but the
 * file it names, its input (the text it replaces with a line around it,
 * as Claude Code's own edits have, and the text that goes there) and the
 * lines it changes. Its first row, while it runs, has the file and input. */
function claudeEdit(path: string, root: string): { running: Partial<ToolEvent>; done: Partial<ToolEvent> } {
  const file = `${root}/${path}`;
  const before = (MAIN[path] ?? "").split("\n");
  const after = (changedIn("thread")[path] ?? "").split("\n");
  let head = 0;
  while (head < before.length && head < after.length && before[head] === after[head]) head += 1;
  let tail = 0;
  while (
    tail < before.length - head &&
    tail < after.length - head &&
    before[before.length - 1 - tail] === after[after.length - 1 - tail]
  )
    tail += 1;
  const from = Math.max(0, head - 1);
  const input = {
    file_path: file,
    old_string: before.slice(from, Math.min(before.length, before.length - tail + 1)).join("\n"),
    new_string: after.slice(from, Math.min(after.length, after.length - tail + 1)).join("\n"),
  };
  const counts = THREAD_CHANGES.find((f) => f.path === path);
  const running = { detail: JSON.stringify(input, null, 2), paths: [file] };
  // Claude Code's result: the new text's lines, numbered as `cat -n` does.
  const snippet = input.new_string
    .split("\n")
    .map((line, i) => `${String(from + i + 1).padStart(6)}→${line}`);
  return {
    running,
    done: {
      ...running,
      output: [
        `The file ${file} has been updated. Here's the result of running \`cat -n\` on a snippet of the edited file:`,
        ...snippet,
      ].join("\n"),
      lines: { added: counts?.additions ?? 0, removed: counts?.deletions ?? 0 },
    },
  };
}

/** The command the fix asks to run, and what it prints once the fix is in. */
export const TEST_COMMAND = "pnpm test auth";
export const TEST_OUTPUT = testRun({ ...MAIN, ...changedIn("thread") }, "auth").join("\n");
const TESTS_PASSED = /Tests {2}(\d+) passed/.exec(TEST_OUTPUT)?.[1] ?? "0";

const usage = (input: number, output: number, fill: number): ThreadEvent => ({
  type: "usage",
  input,
  output,
  cacheRead: 0,
  cacheWrite: 0,
  contextFillPct: fill,
  costUsd: null,
});

/** A turn that reads, edits, asks to run the tests, and reports back. The
 * first turn of the demo, whatever the visitor typed, in `root` (shop's
 * folder). */
export function fixTurn(approvalId: string, expiresAt: number, root: string): Step[] {
  const session = claudeEdit(SESSION, root);
  const sessionTest = claudeEdit(SESSION_TEST, root);
  return [
    {
      wait: 500,
      event: {
        type: "reasoning",
        text: "The loop starts when a session expires while another tab still holds its cookie: getSession drops it, checkout sends that tab to /login, and signing in again only fixes the tab you sign in from.",
        durationMs: 1800,
      },
    },
    { wait: 600, event: tool("t1", "grep", "getSession src/", "running") },
    { wait: 500, event: tool("t1", "grep", "getSession src/", "ok", { output: grep(MAIN, "getSession").join("\n") }) },
    { wait: 300, event: tool("t2", "read", SESSION, "running") },
    { wait: 500, event: tool("t2", "read", SESSION, "ok", { output: `(${lineCount(MAIN[SESSION] ?? "")} lines)` }) },
    { wait: 700, event: tool("t3", "edit", SESSION, "running", session.running) },
    { wait: 500, event: tool("t3", "edit", SESSION, "ok", session.done) },
    { wait: 400, event: tool("t4", "edit", SESSION_TEST, "running", sessionTest.running) },
    { wait: 500, event: tool("t4", "edit", SESSION_TEST, "ok", sessionTest.done) },
    {
      approval: {
        type: "approval_request",
        id: approvalId,
        kind: "command",
        summary: `Run \`${TEST_COMMAND}\``,
        detail: TEST_COMMAND,
        options: ["once", "thread"],
        expiresAt,
      },
    },
  ];
}

/** After Allow: the tests run and the agent wraps up. */
export function afterAllow(): Step[] {
  return [
    { wait: 300, event: tool("t5", "bash", TEST_COMMAND, "running") },
    { wait: 1400, event: tool("t5", "bash", TEST_COMMAND, "ok", { output: TEST_OUTPUT }) },
    {
      wait: 600,
      event: {
        type: "assistant",
        markdown: [
          "Fixed the redirect loop.",
          "",
          "- **Cause:** a session that expired while another tab still had its cookie was dropped, so checkout sent that tab to `/login`, again and again.",
          `- **Fix:** \`getSession\` now renews a session that expired less than 15 minutes ago (\`${SESSION}\`).`,
          `- **Tests:** \`${SESSION_TEST}\` covers the renewal now; all ${TESTS_PASSED} tests pass.`,
          "",
          "Open **Changes** to review the diff.",
        ].join("\n"),
      },
    },
    { wait: 100, event: usage(18_420, 1_260, 9.2) },
  ];
}

/** After Deny: nothing runs. */
export function afterDeny(): Step[] {
  return [
    {
      wait: 500,
      event: {
        type: "assistant",
        markdown: `Understood, I won't run the tests. The fix is in \`${SESSION}\`, with its tests in \`${SESSION_TEST}\`; run \`${TEST_COMMAND}\` when you're ready.`,
      },
    },
    { wait: 100, event: usage(16_900, 640, 8.4) },
  ];
}

/** Any later message: a short streamed answer. */
export function chatTurn(text: string): Step[] {
  const lower = text.toLowerCase();
  const reply = lower.includes("plan")
    ? "Here's a plan:\n\n1. Reproduce the bug with a failing test.\n2. Fix the smallest piece of code that causes it.\n3. Run the related tests, then the whole suite.\n\nSwitch the composer to **Act** when you want me to start."
    : lower.includes("test")
      ? "In the real app I'd run that in your project, asking you first if the command isn't allowed yet. In this demo the agent is simulated, so nothing runs."
      : `This is a simulated agent, so I can't really work on "${text.slice(0, 80)}". In Vibezeo the message goes to Claude Code, Codex or another agent you've installed, running on your own machine and your own plan.\n\nDownload the app for Linux to try it on your code.`;
  return [
    { wait: 400, event: { type: "reasoning", text: "Answering the question.", durationMs: 600 } },
    { wait: 500, event: { type: "assistant", markdown: reply } },
    { wait: 100, event: usage(4_200 + text.length * 3, 180, 3.1) },
  ];
}

/** A Plan-mode answer: numbered steps for what was asked (F-TH-10). */
export function planText(text: string): string {
  const what = text.trim().replace(/\s+/g, " ").slice(0, 80) || "the change";
  return [
    `1. Find the code behind "${what}" and the tests that cover it.`,
    "2. Write a failing test for the new behaviour.",
    "3. Make the smallest change that passes it.",
    "4. Run the related tests, then the whole suite.",
  ].join("\n");
}

export function planTurn(plan: string): Step[] {
  return [
    { wait: 400, event: { type: "reasoning", text: "Planning only: nothing is edited in Plan mode.", durationMs: 700 } },
    { wait: 500, event: { type: "assistant", markdown: plan } },
    { wait: 100, event: usage(5_100, 220, 3.4) },
  ];
}
