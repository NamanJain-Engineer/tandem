import { create } from "zustand";
import type {
  EngineView,
  HostEvent,
  HostState,
  HostView,
  IpcError,
  RemoteDir,
  SshProgress,
  SshPrompt,
} from "@tandem/protocol";
import { listen } from "../ipc/events";
import { invoke } from "../ipc/invoke";

/** A host's connection attempt, as `remote:ssh` reports it. */
export type Attempt = {
  opId: string;
  hostId: string;
  stage: string;
  error: IpcError | null;
};

/** An open question from ssh: a new host key or a password. */
export type Ask = { opId: string; hostId: string; prompt: SshPrompt };

export type NewHost = {
  target: string;
  port?: number | null;
  user?: string | null;
  identityFile?: string | null;
  label?: string | null;
};

type RemoteState = {
  hosts: HostView[];
  loaded: boolean;
  error: string | null;
  /** The latest attempt per host. */
  attempts: Record<string, Attempt>;
  ask: Ask | null;
  /** Engines per host (the + menu of a remote workspace). */
  engines: Record<string, EngineView[]>;
  refresh: () => Promise<void>;
  /** The new (or existing) host's id. */
  add: (h: NewHost) => Promise<string>;
  remove: (hostId: string) => Promise<IpcError | null>;
  connect: (hostId: string) => Promise<void>;
  disconnect: (hostId: string) => Promise<void>;
  resetKey: (hostId: string) => Promise<void>;
  answerHostKey: (accept: boolean) => void;
  answerPassword: (secret: string | null) => void;
  listDir: (hostId: string, path: string) => Promise<RemoteDir>;
  loadEngines: (hostId: string) => Promise<EngineView[]>;
  hostOf: (hostId: string | null | undefined) => HostView | undefined;
};

/** Connected ones can open folders; the rest connect first. */
export const isUp = (s: HostState | undefined) => s === "connected";

export const useRemote = create<RemoteState>((set, get) => ({
  hosts: [],
  loaded: false,
  error: null,
  attempts: {},
  ask: null,
  engines: {},
  refresh: async () => {
    try {
      const hosts = await invoke<HostView[]>("ssh_hosts_list");
      set({ hosts, loaded: true, error: null });
    } catch (e) {
      set({ loaded: true, error: (e as IpcError).message ?? "" });
    }
  },
  add: async (h) => {
    const host = await invoke<{ id: string }>("ssh_host_add", {
      target: h.target,
      port: h.port ?? null,
      user: h.user ?? null,
      identityFile: h.identityFile ?? null,
      label: h.label ?? null,
    });
    await get().refresh();
    return host.id;
  },
  remove: async (hostId) => {
    try {
      await invoke("ssh_host_remove", { hostId });
      await get().refresh();
      return null;
    } catch (e) {
      return e as IpcError;
    }
  },
  connect: async (hostId) => {
    const { opId } = await invoke<{ opId: string }>("ssh_connect", { hostId });
    set((s) => ({
      attempts: { ...s.attempts, [hostId]: { opId, hostId, stage: "resolving", error: null } },
    }));
  },
  disconnect: async (hostId) => {
    await invoke("ssh_disconnect", { hostId }, { timeoutMs: 30_000 });
    await get().refresh();
  },
  resetKey: async (hostId) => {
    await invoke("ssh_host_reset_key", { hostId }, { timeoutMs: 30_000 });
    set((s) => {
      const attempts = { ...s.attempts };
      delete attempts[hostId];
      return { attempts };
    });
    await get().refresh();
  },
  answerHostKey: (accept) => {
    const ask = get().ask;
    if (!ask) return;
    set({ ask: null });
    void invoke("ssh_hostkey_decide", { opId: ask.opId, accept });
  },
  answerPassword: (secret) => {
    const ask = get().ask;
    if (!ask) return;
    set({ ask: null });
    void invoke("ssh_askpass_reply", { opId: ask.opId, secret });
  },
  listDir: (hostId, path) => invoke<RemoteDir>("ssh_list_dir", { hostId, path }, { timeoutMs: 30_000 }),
  loadEngines: async (hostId) => {
    const list = await invoke<EngineView[]>("engines_list", { hostId }).catch(() => [] as EngineView[]);
    set((s) => ({ engines: { ...s.engines, [hostId]: list } }));
    return list;
  },
  hostOf: (hostId) => get().hosts.find((h) => h.id === hostId),
}));

let following = false;

/** Keeps hosts, attempts and prompts current. */
export function followRemote(): void {
  if (following) return;
  following = true;
  void listen<SshProgress>("remote:ssh", (p) => {
    useRemote.setState((s) => ({
      attempts: {
        ...s.attempts,
        [p.hostId]: { opId: p.opId, hostId: p.hostId, stage: p.stage, error: p.error },
      },
      ask: p.prompt ? { opId: p.opId, hostId: p.hostId, prompt: p.prompt } : s.ask?.opId === p.opId ? null : s.ask,
    }));
    if (p.stage === "connected" || p.stage === "failed") void useRemote.getState().refresh();
  });
  void listen<HostEvent>("remote:host", ({ hostId, state }) => {
    useRemote.setState((s) => ({ hosts: s.hosts.map((h) => (h.id === hostId ? { ...h, state } : h)) }));
  });
  void listen<{ hostId: string }>("engines:detected", ({ hostId }) => {
    if (hostId !== "local") void useRemote.getState().loadEngines(hostId);
  });
  void useRemote.getState().refresh();
}
