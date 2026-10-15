import type {
  EngineReads,
  InstructionsChange,
  InstructionsFile,
  InstructionsPlan,
  InstructionsReport,
  IpcError,
  SetupState,
} from "@tandem/protocol";
import { invoke } from "../ipc/invoke";
import type { Outcome } from "./launch";

async function run<T>(cmd: string, args: Record<string, unknown>): Promise<Outcome<T>> {
  try {
    return { ok: true, value: await invoke<T>(cmd, args) };
  } catch (e) {
    return { ok: false, error: e as IpcError };
  }
}

export const AGENTS = "AGENTS.md";

/** One instructions file for every engine (F-EN-07). */
export const getInstructions = (workspaceId: string) =>
  run<InstructionsReport>("instructions_get", { workspaceId });
export const planInstructions = (workspaceId: string, source: string | null) =>
  run<InstructionsPlan>("instructions_plan", { workspaceId, source });
export const applyInstructions = (workspaceId: string, source: string | null, digest: string) =>
  run<InstructionsChange>("instructions_apply", { workspaceId, source, digest });
export const undoInstructions = (changeId: string) => run<null>("instructions_undo", { changeId });
export const copyInstructionsToWorktrees = (workspaceId: string, path: string) =>
  run<SetupState>("instructions_copy_to_worktrees", { workspaceId, path });

/** What an engine reads, in a few words. */
export type ReadsSummary =
  | { kind: "unknown" }
  | { kind: "nothing" }
  | { kind: "agents"; others: string[] }
  | { kind: "other"; files: string[]; notAgents: boolean };

export function readsSummary(e: EngineReads, agentsExists: boolean): ReadsSummary {
  if (!e.known) return { kind: "unknown" };
  if (e.reads.length === 0) return { kind: "nothing" };
  const others = e.reads.filter((f) => f !== AGENTS);
  if (e.readsAgents) return { kind: "agents", others };
  return { kind: "other", files: others, notAgents: agentsExists };
}

/** The files that exist, `AGENTS.md` first. */
export const existing = (files: InstructionsFile[]) => files.filter((f) => f.exists);

/** Files that can become `AGENTS.md` when there's none. */
export const sources = (files: InstructionsFile[]) =>
  files.filter((f) => f.exists && f.path !== AGENTS && !f.path.endsWith("/") && !f.unsafe && !f.pointsToAgents);
