import type { ApprovalView } from "@tandem/protocol";

/** What a `vibezeo run` approval's detail holds (F-TL-12): the exact line,
 * where it runs and every variable it gets from `.env`. */
export type RunApproval = {
  command: string;
  cwd: string;
  vars: Array<{ name: string; secret: string | null }>;
  /** A rule covers the command, but `.env` changed since it was allowed. */
  envChanged: boolean;
};

const isVar = (v: unknown): v is { name: string; secret: string | null } =>
  typeof v === "object" &&
  v !== null &&
  typeof (v as { name?: unknown }).name === "string" &&
  ((v as { secret?: unknown }).secret === null || typeof (v as { secret?: unknown }).secret === "string");

/** A pane's run, read from its approval. Only a pane's approval can be one:
 * a thread's detail is the model's own tool input, so it could pose as a run. */
export function parseRunApproval(approval: Pick<ApprovalView, "source" | "detail">): RunApproval | null {
  if (approval.source.type !== "pane") return null;
  try {
    const run = (JSON.parse(approval.detail) as { vibezeoRun?: Record<string, unknown> }).vibezeoRun;
    if (
      !run ||
      typeof run.command !== "string" ||
      typeof run.cwd !== "string" ||
      typeof run.envChanged !== "boolean" ||
      !Array.isArray(run.vars) ||
      !run.vars.every(isVar)
    )
      return null;
    return { command: run.command, cwd: run.cwd, vars: run.vars, envChanged: run.envChanged };
  } catch {
    return null;
  }
}

/** The line in pieces, every character outside printable ASCII as its code
 * point, so look-alike letters and right-to-left text can't pass as ASCII. */
export function markLine(line: string): Array<{ text: string; marked: boolean }> {
  const out: Array<{ text: string; marked: boolean }> = [];
  for (const ch of line) {
    const code = ch.codePointAt(0) ?? 0;
    const marked = code < 0x20 || code > 0x7e;
    const text = marked ? `\\u{${code.toString(16).toUpperCase()}}` : ch;
    const last = out[out.length - 1];
    if (last && !marked && !last.marked) last.text += text;
    else out.push({ text, marked });
  }
  return out;
}
