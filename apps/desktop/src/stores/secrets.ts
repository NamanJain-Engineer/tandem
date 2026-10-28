import { create } from "zustand";
import type { AgentsMdPlan, IpcError, PaneRecord, RunPlan, RunRuleRepo, SecretInfo } from "@tandem/protocol";
import { invoke } from "../ipc/invoke";

/** Settings › Secrets (F-TL-12): names only; values never reach the webview
 * after they're saved. */
type SecretsState = {
  list: SecretInfo[];
  state: "idle" | "loading" | "ready" | "error";
  error: string | null;
  load: () => Promise<void>;
  save: (name: string, value: string) => Promise<void>;
  remove: (name: string) => Promise<void>;
};

export const useSecrets = create<SecretsState>((set, get) => ({
  list: [],
  state: "idle",
  error: null,
  load: async () => {
    set({ state: get().state === "ready" ? "ready" : "loading", error: null });
    try {
      const list = (await invoke<SecretInfo[] | null>("secrets_list", {})) ?? [];
      set({ list, state: "ready" });
    } catch (e) {
      set({ state: "error", error: (e as IpcError).message ?? String(e) });
    }
  },
  save: async (name, value) => {
    await invoke<SecretInfo>("secrets_set", { name, value }, { timeoutMs: 30_000 });
    await get().load();
  },
  remove: async (name) => {
    await invoke("secrets_delete", { name }, { timeoutMs: 30_000 });
    await get().load();
  },
}));

/** `vibezeo-secret://stripe-test`. */
export function reference(name: string): string {
  return `vibezeo-secret://${name}`;
}

/** Lowercase letters, digits, - and _, up to 64, starting with a letter or digit. */
export function validName(name: string): boolean {
  return /^[a-z0-9][a-z0-9_-]{0,63}$/.test(name);
}

export function runPlan(paneId: string): Promise<RunPlan | null> {
  return invoke<RunPlan | null>("secrets_run_plan", { paneId });
}

export function runWithSecrets(paneId: string, tabId: string, command: string): Promise<PaneRecord> {
  return invoke<PaneRecord>("secrets_run", { paneId, tabId, command }, { timeoutMs: 30_000 });
}

/** What "Tell agents in this folder" would write to the pane's folder's AGENTS.md. */
export function agentsPlan(paneId: string): Promise<AgentsMdPlan> {
  return invoke<AgentsMdPlan>("secrets_agents_plan", { paneId });
}

/** Writes it, if AGENTS.md is still `before`. */
export function agentsApply(paneId: string, before: string): Promise<AgentsMdPlan> {
  return invoke<AgentsMdPlan>("secrets_agents_apply", { paneId, before });
}

/** Commands agents may run with secrets without asking, per folder (F-TL-12). */
export function runRulesList(): Promise<RunRuleRepo[]> {
  return invoke<RunRuleRepo[] | null>("run_rules_list", {}).then((l) => l ?? []);
}

export function runRulesSet(repo: string, commands: string[]): Promise<RunRuleRepo> {
  return invoke<RunRuleRepo>("run_rules_set", { repo, commands });
}
