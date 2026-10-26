import type { ToolStatus, ToolVerb } from "@tandem/protocol";
import type { ToolIcon } from "@tandem/ui";
import { t, type MessageKey } from "../../i18n";
import type { ToolEvent } from "./rows";

const ICON: Record<ToolVerb, ToolIcon> = {
  bash: "command",
  read: "read",
  grep: "search",
  glob: "search",
  edit: "edit",
  write: "edit",
  multiedit: "edit",
  webfetch: "web",
  websearch: "web",
  task: "agent",
  mcp: "plugin",
  other: "other",
};

/** `+a −d` of a unified diff (headers excluded). */
export function diffCounts(diff: string): { add: number; del: number } {
  let add = 0;
  let del = 0;
  for (const line of diff.split("\n")) {
    if (line.startsWith("+++") || line.startsWith("---")) continue;
    if (line.startsWith("+")) add++;
    else if (line.startsWith("-")) del++;
  }
  return { add, del };
}

export type TestResult = { passed: number; failed: number };

const count = (line: string, word: string) => Number(new RegExp(`(\\d+) ${word}`).exec(line)?.[1] ?? 0);

/** The test summary a command printed (D-2604): vitest's and jest's
 * "Tests" line (the last one), cargo's "test result" lines (summed), or
 * pytest's closing line. Nothing when the output carries none. */
export function testResult(output: string | null | undefined): TestResult | null {
  if (!output) return null;
  const cargo = [...output.matchAll(/^test result: \w+\. (\d+) passed; (\d+) failed/gm)];
  if (cargo.length > 0)
    return cargo.reduce((r, m) => ({ passed: r.passed + Number(m[1]), failed: r.failed + Number(m[2]) }), {
      passed: 0,
      failed: 0,
    });
  const tests = [...output.matchAll(/^\s*Tests:?\s+(.*\d+ (?:passed|failed).*)$/gm)].pop();
  if (tests?.[1]) return { passed: count(tests[1], "passed"), failed: count(tests[1], "failed") };
  const pytest = [...output.matchAll(/^=+ (.*\d+ (?:passed|failed).*) in [\d.]+s =+$/gm)].pop();
  if (pytest?.[1]) return { passed: count(pytest[1], "passed"), failed: count(pytest[1], "failed") };
  return null;
}

const STATUS_KEY: Record<ToolStatus, MessageKey> = {
  pending: "thread.tool.status.running",
  running: "thread.tool.status.running",
  ok: "thread.tool.status.ok",
  error: "thread.tool.status.error",
  denied: "thread.tool.status.denied",
};

export function statusLabel(status: ToolStatus): string {
  return t(STATUS_KEY[status]);
}

/** What a ToolCallBlock shows for a tool event (UI-21 labels). */
export function toolView(tool: ToolEvent): {
  icon: ToolIcon;
  verb: string;
  arg: string;
  meta?: string;
  statusLabel: string;
} {
  const active = tool.status === "running" || tool.status === "pending";
  const verb = t(`thread.tool.${tool.verb}${active ? ".active" : ""}` as MessageKey);
  let meta: string | undefined;
  const tests = tool.verb === "bash" && !active ? testResult(tool.output) : null;
  if (tool.status === "denied") meta = t("thread.tool.denied");
  else if (tool.diff) {
    const { add, del } = diffCounts(tool.diff);
    meta = t("thread.tool.diffMeta", { add, del });
  } else if (tests) meta = t("thread.tool.tests", { passed: tests.passed, failed: tests.failed });
  return { icon: ICON[tool.verb], verb, arg: tool.argSummary, meta, statusLabel: statusLabel(tool.status) };
}
