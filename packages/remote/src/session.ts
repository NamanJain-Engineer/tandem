// The phone's screen logic (F-RM-03, F-RM-05), independent of any UI kit: it
// owns a PhoneClient, keeps what the screens show (phase, connection,
// snapshot, grants) and turns snapshot changes into notices (a new approval,
// an agent that finished or needs input). Vibezeo Remote renders it with
// React Native; tests drive it directly.
import { PairingError, PhoneClient, type CommandResult, type ConnectionState, type SocketFactory } from "./client";
import type {
  JsonValue,
  PhoneAgent,
  PhoneApproval,
  PhoneDecision,
  PhoneGrants,
  PhoneSnapshot,
  PhoneTranscript,
} from "./messages";
import { parsePairingFragment, type PairingLink } from "./protocol";
import type { PairingStore } from "./store";
import { DEMO_RELAY } from "./demo";

export type Phase =
  | { kind: "loading" }
  | { kind: "unpaired" }
  | { kind: "pairing" }
  | { kind: "pair_failed"; message: string }
  | { kind: "revoked" }
  | { kind: "paired" };

export type SessionView = {
  phase: Phase;
  connection: ConnectionState;
  desktop: string;
  grants: PhoneGrants | null;
  armedUntil: number | null;
  snapshot: PhoneSnapshot | null;
  snapshotError: string | null;
  /** The last frame the desktop refused, if any (`replay`, `epoch`, `bad_frame`). */
  rejected: string | null;
};

/** Something worth telling the person about, from one snapshot to the next. */
export type Notice =
  | { kind: "approval"; id: string; title: string; body: string }
  | { kind: "agent"; id: string; title: string; body: string };

export type SessionOptions = {
  socketFactory?: SocketFactory;
  device?: string;
  /** Called with each notice a refresh finds (local notifications, F-RM-05). */
  onNotice?: (notice: Notice) => void;
  /** Demo mode: the only relay allowed is the demo computer's. */
  demo?: boolean;
};

const NO_GRANTS: PhoneGrants = { chat: false, terminal: false, approvals: false, routines: false };

/** Reads a pairing link from a scanned code or an opened URL (any host, or `vibezeo-remote://pair#p=…`). */
export function linkFromUrl(url: string): PairingLink | null {
  const hash = url.indexOf("#");
  if (hash >= 0) {
    const fromHash = parsePairingFragment(url.slice(hash));
    if (fromHash) return fromHash;
  }
  const query = url.indexOf("?");
  if (query >= 0) return parsePairingFragment(`#${url.slice(query + 1).split("#")[0] ?? ""}`);
  return null;
}

/**
 * Whether a relay URL is one the phone may connect to: `wss://` anywhere,
 * `ws://` only to a private, VPN or loopback address (LAN mode, F-RM-04).
 * The frames are end-to-end encrypted either way; this keeps a code from
 * pointing the phone at a plain-text server on the internet.
 */
export function relayAllowed(relay: string): boolean {
  let url: URL;
  try {
    url = new URL(relay);
  } catch {
    return false;
  }
  if (url.protocol === "wss:") return true;
  if (url.protocol !== "ws:") return false;
  const m = /^(\d+)\.(\d+)\.(\d+)\.(\d+)$/.exec(url.hostname);
  if (!m) return false;
  const [a, b] = [Number(m[1]), Number(m[2])];
  return (
    a === 10 ||
    a === 127 ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 100 && b >= 64 && b <= 127) ||
    (a === 169 && b === 254)
  );
}

function agentNotice(prev: PhoneAgent | undefined, next: PhoneAgent): Notice | null {
  if (!prev || prev.status === next.status) return null;
  if (next.status === "waiting") return { kind: "agent", id: next.key, title: `${next.title} needs you`, body: next.preview ?? "" };
  if (next.status === "done" && prev.status === "working")
    return { kind: "agent", id: next.key, title: `${next.title} finished`, body: next.preview ?? "" };
  if (next.status === "failed") return { kind: "agent", id: next.key, title: `${next.title} stopped with an error`, body: next.preview ?? "" };
  return null;
}

/** What changed between two snapshots that the person should hear about. */
export function noticesBetween(prev: PhoneSnapshot | null, next: PhoneSnapshot): Notice[] {
  if (!prev) return [];
  const out: Notice[] = [];
  const seen = new Set(prev.approvals.map((a) => a.id));
  for (const a of next.approvals) {
    if (!seen.has(a.id) && !a.queued) out.push({ kind: "approval", id: a.id, title: "Approval needed", body: a.summary });
  }
  const before = new Map(prev.agents.map((a) => [a.key, a]));
  for (const a of next.agents) {
    const n = agentNotice(before.get(a.key), a);
    if (n) out.push(n);
  }
  return out;
}

export class RemoteSession {
  private readonly client: PhoneClient;
  private readonly listeners = new Set<() => void>();
  private view: SessionView = {
    phase: { kind: "loading" },
    connection: "idle",
    desktop: "",
    grants: null,
    armedUntil: null,
    snapshot: null,
    snapshotError: null,
    rejected: null,
  };

  constructor(
    store: PairingStore,
    private readonly options: SessionOptions = {},
  ) {
    this.client = new PhoneClient(
      store,
      {
        state: (connection) => {
          this.set({ connection });
          if (connection === "revoked") this.set({ phase: { kind: "revoked" }, snapshot: null });
          if (connection === "online") void this.refresh();
        },
        welcome: (desktop, grants, armedUntil) => this.set({ desktop, grants, armedUntil }),
        changed: () => void this.refresh(),
        rejected: (rejected) => this.set({ rejected }),
      },
      options.socketFactory,
      options.device ?? "Phone",
    );
  }

  /** The current view (stable between changes, for `useSyncExternalStore`). */
  get current(): SessionView {
    return this.view;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private set(patch: Partial<SessionView>): void {
    this.view = { ...this.view, ...patch };
    for (const l of this.listeners) l();
  }

  /** Resumes a saved pairing, if there is one. */
  async start(): Promise<void> {
    const resumed = await this.client.resume().catch(() => false);
    const paired = this.client.paired;
    if (resumed && paired) this.set({ phase: { kind: "paired" }, desktop: paired.desktop, grants: paired.grants });
    else this.set({ phase: { kind: "unpaired" } });
  }

  /** Pairs from a scanned or opened link. */
  async pairUrl(url: string): Promise<boolean> {
    const link = linkFromUrl(url);
    if (!link) {
      this.set({ phase: { kind: "pair_failed", message: "That isn't a pairing code. Make one in Settings › Remote on your computer." } });
      return false;
    }
    return this.pair(link);
  }

  async pair(link: PairingLink): Promise<boolean> {
    if (this.options.demo ? link.relay !== DEMO_RELAY : !relayAllowed(link.relay)) {
      this.set({ phase: { kind: "pair_failed", message: "This code points somewhere the app won't connect to." } });
      return false;
    }
    this.set({ phase: { kind: "pairing" }, snapshot: null, snapshotError: null, rejected: null });
    try {
      const p = await this.client.pair(link);
      this.set({ phase: { kind: "paired" }, desktop: p.desktop, grants: p.grants });
      return true;
    } catch (err) {
      this.set({
        phase: {
          kind: "pair_failed",
          message: err instanceof PairingError ? err.message : "Pairing didn't work. Make a new code and try again.",
        },
      });
      return false;
    }
  }

  /** Fetches a fresh snapshot (and reports what changed). */
  async refresh(): Promise<void> {
    if (this.client.connection !== "online") return;
    const r = await this.client.command<PhoneSnapshot>("view.snapshot");
    if (!r.ok) {
      this.set({ snapshotError: r.message });
      return;
    }
    const prev = this.view.snapshot;
    this.set({ snapshot: r.data, grants: r.data.grants, armedUntil: r.data.armedUntil ?? null, snapshotError: null });
    for (const n of noticesBetween(prev, r.data)) this.options.onNotice?.(n);
  }

  /** Allow or deny; an allow the desktop hasn't armed comes back `queued`. */
  async decide(approval: PhoneApproval, decision: PhoneDecision): Promise<CommandResult<{ state?: string }>> {
    const r = await this.client.command<{ state?: string }>("approval.decide", { approvalId: approval.id, decision });
    void this.refresh();
    return r;
  }

  readThread(threadId: string, before?: number | null): Promise<CommandResult<PhoneTranscript>> {
    const args: { [key: string]: JsonValue } = { threadId, limit: 40 };
    if (before != null) args.before = before;
    return this.client.command<PhoneTranscript>("thread.read", args);
  }

  sendToThread(threadId: string, text: string): Promise<CommandResult> {
    return this.client.command("thread.send", { threadId, text });
  }

  /** Types into an agent pane (never a plain shell; the desktop checks). */
  typeToPane(paneId: string, text: string): Promise<CommandResult> {
    return this.client.command("pane.type", { paneId, text });
  }

  async runRoutine(routineId: string): Promise<CommandResult> {
    const r = await this.client.command("routine.run_now", { routineId });
    void this.refresh();
    return r;
  }

  /** F-RM-06: gives the desktop this phone's push token and alert key. */
  registerPush(platform: "fcm" | "apns", token: string, key: string): Promise<CommandResult> {
    return this.client.command("push.register", { platform, token, key });
  }

  unregisterPush(): Promise<CommandResult> {
    return this.client.command("push.unregister");
  }

  /** The pairing's id and relay (push needs the hosted relay, F-RM-06). */
  get pairing(): { pid: string; relay: string } | null {
    const p = this.client.paired;
    return p ? { pid: p.link.pid, relay: p.link.relay } : null;
  }

  async unpair(): Promise<void> {
    await this.client.unpair();
    this.set({ phase: { kind: "unpaired" }, snapshot: null, desktop: "", grants: null });
  }

  /** Reconnects after the app comes back to the foreground. */
  resume(): void {
    if (this.view.phase.kind === "paired" && this.client.connection === "idle") void this.client.resume();
  }

  /** Drops the socket (app in the background, or closing). */
  stop(): void {
    this.client.disconnect();
  }

  get grants(): PhoneGrants {
    return this.view.grants ?? NO_GRANTS;
  }
}
