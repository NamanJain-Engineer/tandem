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
export function toolView(tool: ToolEvent): { icon: ToolIcon; verb: string; arg: string; meta?: string; statusLabel: string } {
  const active = tool.status === "running" || tool.status === "pending";
  const verb = t(`thread.tool.${tool.verb}${active ? ".active" : ""}` as MessageKey);
  let meta: string | undefined;
  if (tool.status === "denied") meta = t("thread.tool.denied");
  else if (tool.diff) {
    const { add, del } = diffCounts(tool.diff);
    meta = t("thread.tool.diffMeta", { add, del });
  }
  return { icon: ICON[tool.verb], verb, arg: tool.argSummary, meta, statusLabel: statusLabel(tool.status) };
}
