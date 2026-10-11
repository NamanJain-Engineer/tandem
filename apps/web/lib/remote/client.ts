// The phone's side of a pairing (F-RM-03, ARCHITECTURE §13.3): the QR
// handshake over the relay, sealed commands with replies, rekeying and
// revocation. The PWA uses it with IndexedDB; E2E runs drive it from Node.
import type { FromPhone, JsonValue, PhoneGrants, ToPhone } from "./messages";
import {
  acceptRekey,
  b64url,
  decodeJson,
  encodeHello,
  encodeJson,
  fromB64url,
  generateKeyPair,
  helloTag,
  newSession,
  openNext,
  ProtocolError,
  rootSecret,
  sealNext,
  type PairingLink,
  type SessionState,
} from "./protocol";
import type { PairingStore, StoredPairing } from "./store";

export type ConnectionState =
  | "idle"
  | "connecting"
  /** Our socket is open; the desktop's isn't. */
  | "desktop_offline"
  | "online"
  | "revoked"
  /** Another tab or device took this pairing's phone socket. */
  | "replaced";

export type CommandResult<T = unknown> = { ok: true; data: T } | { ok: false; code: string; message: string };

type SocketLike = {
  binaryType: string;
  readyState: number;
  protocol: string;
  send(data: Uint8Array): void;
  close(code?: number, reason?: string): void;
  onopen: ((ev: unknown) => void) | null;
  onclose: ((ev: { code: number; reason: string }) => void) | null;
  onerror: ((ev: unknown) => void) | null;
  onmessage: ((ev: { data: unknown }) => void) | null;
};
export type SocketFactory = (url: string, protocols: string[]) => SocketLike;

export const browserSocket: SocketFactory = (url, protocols) => new WebSocket(url, protocols) as unknown as SocketLike;

export type ClientEvents = {
  state?: (state: ConnectionState) => void;
  welcome?: (desktop: string, grants: PhoneGrants, armedUntil: number | null) => void;
  changed?: () => void;
  rejected?: (reason: string) => void;
};

const COMMAND_TIMEOUT_MS = 15_000;
const PAIR_TIMEOUT_MS = 20_000;

export class PairingError extends Error {}

export class PhoneClient {
  private socket: SocketLike | null = null;
  private pairing: StoredPairing | null = null;
  private nextId = 1;
  private readonly waiting = new Map<number, (r: CommandResult) => void>();
  private welcomeWaiter: ((ok: boolean) => void) | null = null;
  private state: ConnectionState = "idle";
  private closedByUs = false;
  private retry: ReturnType<typeof setTimeout> | null = null;
  private retryMs = 1000;
  private hello: Uint8Array | null = null;
  /** The last frame sent, so tests can replay it. */
  lastFrame: Uint8Array | null = null;

  constructor(
    private readonly store: PairingStore,
    private readonly events: ClientEvents = {},
    private readonly socketFactory: SocketFactory = browserSocket,
    private readonly device = "Phone",
  ) {}

  get connection(): ConnectionState {
    return this.state;
  }

  get paired(): StoredPairing | null {
    return this.pairing;
  }

  /** Loads a stored pairing and connects. False when there's none. */
  async resume(): Promise<boolean> {
    this.pairing = await this.store.load();
    if (!this.pairing) return false;
    this.connect();
    return true;
  }

  /** Pairs with the desktop whose QR link this is, then stays connected. */
  async pair(link: PairingLink): Promise<StoredPairing> {
    this.disconnect();
    const keys = generateKeyPair();
    const s = fromB64url(link.s);
    const dpk = fromB64url(link.dpk);
    const root = rootSecret(s, link.pid, dpk, keys.public, keys.secret, dpk);
    const hello = encodeHello(keys.public, helloTag(s, link.pid, keys.public));
    const { s: _secret, ...kept } = link;
    const candidate: StoredPairing = {
      link: kept,
      phoneSecret: keys.secret,
      session: newSession(link.pid, "phone", root),
      desktop: "",
      grants: { chat: false, terminal: false, approvals: false, routines: false },
      pairedAt: Date.now(),
    };
    this.pairing = candidate;
    const welcomed = new Promise<boolean>((resolve) => {
      this.welcomeWaiter = resolve;
      setTimeout(() => resolve(false), PAIR_TIMEOUT_MS);
    });
    this.hello = hello;
    this.connect();
    const ok = await welcomed;
    this.welcomeWaiter = null;
    this.hello = null;
    if (!ok || !this.pairing) {
      this.disconnect();
      this.pairing = null;
      throw new PairingError("Pairing didn't finish. The code may have expired: make a new one on your computer.");
    }
    return this.pairing;
  }

  /** Sends a command and waits for the desktop's answer. */
  command<T = unknown>(cmd: string, args: { [key: string]: JsonValue } = {}): Promise<CommandResult<T>> {
    const id = this.nextId++;
    return new Promise<CommandResult<T>>((resolve) => {
      const timer = setTimeout(() => {
        this.waiting.delete(id);
        resolve({ ok: false, code: "timeout", message: "Your computer didn't answer. Check that it's online." });
      }, COMMAND_TIMEOUT_MS);
      this.waiting.set(id, (r) => {
        clearTimeout(timer);
        resolve(r as CommandResult<T>);
      });
      void this.send({ t: "cmd", id, cmd, args }).catch(() => {
        clearTimeout(timer);
        this.waiting.delete(id);
        resolve({ ok: false, code: "offline", message: "Not connected to your computer." });
      });
    });
  }

  /** Unpairs from this side: the desktop revokes the pairing, and this phone forgets it. */
  async unpair(): Promise<void> {
    try {
      await this.send({ t: "bye" });
    } catch {
      // Offline: the desktop still lists it until someone revokes it there.
    }
    await this.forget("revoked");
  }

  /** Sends bytes as they are (E2E: replays and oversized frames). */
  sendRaw(frame: Uint8Array): void {
    this.socket?.send(frame);
  }

  disconnect(): void {
    this.closedByUs = true;
    if (this.retry) clearTimeout(this.retry);
    this.retry = null;
    this.socket?.close(1000, "bye");
    this.socket = null;
    if (this.state !== "revoked") this.setState("idle");
  }

  private setState(state: ConnectionState): void {
    if (this.state === state) return;
    this.state = state;
    this.events.state?.(state);
  }

  private connect(): void {
    const pairing = this.pairing;
    if (!pairing) return;
    this.closedByUs = false;
    this.setState("connecting");
    const ws = this.socketFactory(pairing.link.relay, [`tandem-pair.${pairing.link.ptok}`]);
    ws.binaryType = "arraybuffer";
    this.socket = ws;
    // The relay says right away whether the desktop is there (`peer`); the
    // greeting waits for that.
    ws.onopen = () => {
      this.retryMs = 1000;
    };
    ws.onmessage = (ev) => {
      if (typeof ev.data === "string") this.onRelayText(ev.data);
      else if (ev.data instanceof ArrayBuffer) void this.onFrame(new Uint8Array(ev.data));
      else if (ArrayBuffer.isView(ev.data))
        void this.onFrame(new Uint8Array(ev.data.buffer, ev.data.byteOffset, ev.data.byteLength));
    };
    ws.onclose = (ev) => {
      if (this.socket !== ws) return;
      this.socket = null;
      for (const resolve of this.waiting.values()) {
        resolve({ ok: false, code: "offline", message: "The connection to your computer dropped." });
      }
      this.waiting.clear();
      if (ev.code === 1008 && !this.welcomeWaiter) {
        void this.forget("revoked");
        return;
      }
      if (ev.code === 4000) {
        this.setState("replaced");
        return;
      }
      if (this.welcomeWaiter) {
        this.welcomeWaiter(false);
        return;
      }
      if (this.closedByUs) return;
      this.setState("connecting");
      this.retry = setTimeout(() => this.connect(), this.retryMs);
      this.retryMs = Math.min(this.retryMs * 2, 30_000);
    };
    ws.onerror = () => undefined;
  }

  /** The relay drops frames while the desktop is away, so greet again when it arrives. */
  private greet(): void {
    if (this.hello) this.socket?.send(this.hello);
    void this.send({ t: "hi", device: this.device }).catch(() => undefined);
  }

  private onRelayText(text: string): void {
    let msg: { type?: string; up?: boolean };
    try {
      msg = JSON.parse(text) as { type?: string; up?: boolean };
    } catch {
      return;
    }
    if (msg.type !== "peer") return;
    if (msg.up) {
      // "online" waits for the desktop's welcome.
      this.greet();
    } else this.setState("desktop_offline");
  }

  private async send(message: FromPhone): Promise<void> {
    const pairing = this.pairing;
    const ws = this.socket;
    if (!pairing || !ws || ws.readyState !== 1) throw new Error("offline");
    const sealed = sealNext(pairing.session, encodeJson(message));
    pairing.session = sealed.state;
    // The counter is saved before the frame leaves, so a reload never reuses it.
    if (!this.welcomeWaiter) await this.store.save(pairing);
    this.lastFrame = sealed.frame;
    ws.send(sealed.frame);
  }

  private async onFrame(frame: Uint8Array): Promise<void> {
    const pairing = this.pairing;
    if (!pairing) return;
    let opened: { state: SessionState; plaintext: Uint8Array };
    try {
      opened = openNext(pairing.session, frame);
    } catch (err) {
      this.events.rejected?.(err instanceof ProtocolError ? err.reason : "bad_frame");
      return;
    }
    pairing.session = opened.state;
    let msg: ToPhone;
    try {
      msg = decodeJson<ToPhone>(opened.plaintext);
    } catch {
      return;
    }
    switch (msg.t) {
      case "welcome": {
        pairing.desktop = msg.desktop;
        pairing.grants = msg.grants;
        await this.store.save(pairing);
        this.setState("online");
        this.welcomeWaiter?.(true);
        this.events.welcome?.(msg.desktop, msg.grants, msg.armedUntil ?? null);
        return;
      }
      case "res": {
        await this.store.save(pairing);
        const resolve = this.waiting.get(msg.id);
        this.waiting.delete(msg.id);
        resolve?.(
          msg.ok
            ? { ok: true, data: msg.data ?? null }
            : { ok: false, code: msg.error?.code ?? "error", message: msg.error?.message ?? "That didn't work." },
        );
        return;
      }
      case "rekey": {
        const accepted = acceptRekey(pairing.session, fromB64url(msg.eph));
        pairing.session = accepted.state;
        await this.store.save(pairing);
        await this.send({ t: "rekey_ack", eph: b64url(accepted.eph) });
        return;
      }
      case "changed":
        await this.store.save(pairing);
        this.events.changed?.();
        return;
      case "reject":
        await this.store.save(pairing);
        this.events.rejected?.(msg.reason);
        return;
      case "bye":
        await this.forget("revoked");
        return;
    }
  }

  private async forget(state: "revoked"): Promise<void> {
    this.pairing = null;
    await this.store.clear();
    this.closedByUs = true;
    if (this.retry) clearTimeout(this.retry);
    this.socket?.close(1000, "unpaired");
    this.socket = null;
    this.setState(state);
  }
}
