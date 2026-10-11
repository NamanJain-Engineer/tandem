import { describe, expect, it } from "vitest";
import { PhoneClient, type SocketFactory } from "./client";
import {
  b64url,
  completeRekey,
  decodeHello,
  decodeJson,
  encodeJson,
  equalBytes,
  fromB64url,
  generateKeyPair,
  helloTag,
  newSession,
  openNext,
  rootSecret,
  sealNext,
  type PairingLink,
  type SessionState,
} from "./protocol";
import { deserializePairing, MemoryPairingStore, serializePairing } from "./store";

/** A scripted desktop at the far end of a fake relay socket. */
class FakeDesktop {
  readonly identity = generateKeyPair();
  readonly s = crypto.getRandomValues(new Uint8Array(32));
  readonly pid = "pr_clienttest01";
  session: SessionState | null = null;
  received: unknown[] = [];
  socket: FakeSocket | null = null;
  replied = true;

  link(): PairingLink {
    return {
      v: 1,
      pid: this.pid,
      dpk: b64url(this.identity.public),
      s: b64url(this.s),
      relay: "wss://relay.test/v1/relay/pr_clienttest01",
      ptok: "tok",
    };
  }

  send(message: unknown) {
    if (!this.session || !this.socket) throw new Error("not paired");
    const sealed = sealNext(this.session, encodeJson(message));
    this.session = sealed.state;
    this.socket.deliver(sealed.frame);
  }

  frame(bytes: Uint8Array) {
    if (bytes[0] === 0x01) {
      const { phonePub, tag } = decodeHello(bytes);
      if (!equalBytes(helloTag(this.s, this.pid, phonePub), tag)) return;
      const root = rootSecret(this.s, this.pid, this.identity.public, phonePub, this.identity.secret, phonePub);
      this.session = newSession(this.pid, "desktop", root);
      return;
    }
    if (!this.session) return;
    const opened = openNext(this.session, bytes);
    this.session = opened.state;
    const msg = decodeJson<{ t: string; id?: number; cmd?: string }>(opened.plaintext);
    this.received.push(msg);
    if (!this.replied) return;
    if (msg.t === "hi") this.send({ t: "welcome", desktop: "Work laptop", grants: { chat: true, terminal: false, approvals: false, routines: false } });
    if (msg.t === "cmd") this.send({ t: "res", id: msg.id, ok: true, data: { cmd: msg.cmd } });
  }
}

class FakeSocket {
  binaryType = "blob";
  readyState = 0;
  onopen: ((ev: unknown) => void) | null = null;
  onclose: ((ev: { code: number; reason: string }) => void) | null = null;
  onerror: ((ev: unknown) => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;

  constructor(
    readonly protocol: string,
    private readonly desktop: FakeDesktop,
  ) {
    desktop.socket = this;
    setTimeout(() => {
      this.readyState = 1;
      this.onopen?.({});
      this.onmessage?.({ data: JSON.stringify({ type: "peer", up: true }) });
    }, 0);
  }

  send(data: Uint8Array) {
    queueMicrotask(() => this.desktop.frame(data));
  }

  deliver(frame: Uint8Array) {
    const copy = frame.slice();
    setTimeout(() => this.onmessage?.({ data: copy.buffer }), 0);
  }

  close(code = 1000, reason = "") {
    this.readyState = 3;
    setTimeout(() => this.onclose?.({ code, reason }), 0);
  }
}

function factory(desktop: FakeDesktop, seen: string[][] = []): SocketFactory {
  return (_url, protocols) => {
    seen.push(protocols);
    return new FakeSocket(protocols[0] ?? "", desktop);
  };
}

const tick = () => new Promise((r) => setTimeout(r, 5));

describe("F-RM-03 the phone client", () => {
  it("F-RM-03 pairs from the QR link with the token as subprotocol, and keeps nothing of S", async () => {
    const desktop = new FakeDesktop();
    const store = new MemoryPairingStore();
    const protocols: string[][] = [];
    const client = new PhoneClient(store, {}, factory(desktop, protocols), "Test phone");
    const paired = await client.pair(desktop.link());
    expect(protocols[0]).toEqual(["tandem-pair.tok"]);
    expect(paired.desktop).toBe("Work laptop");
    expect(paired.grants.chat).toBe(true);
    expect(client.connection).toBe("online");
    expect(desktop.received[0]).toEqual({ t: "hi", device: "Test phone" });
    const saved = await store.load();
    expect(saved?.link).not.toHaveProperty("s");
    expect(serializePairing(saved!)).not.toContain(desktop.link().s);
  });

  it("F-RM-03 commands get their answers, and the send counter is saved before each frame", async () => {
    const desktop = new FakeDesktop();
    const store = new MemoryPairingStore();
    const client = new PhoneClient(store, {}, factory(desktop));
    await client.pair(desktop.link());
    const r = await client.command("view.snapshot");
    expect(r).toEqual({ ok: true, data: { cmd: "view.snapshot" } });
    const saved = await store.load();
    expect(saved?.session.txCtr).toBe(2n);
    expect(saved?.session.rxCtr).toBe(2n);
  });

  it("F-RM-03 a reload resumes the pairing and its counters, so the desktop accepts it", async () => {
    const desktop = new FakeDesktop();
    const store = new MemoryPairingStore();
    const first = new PhoneClient(store, {}, factory(desktop));
    await first.pair(desktop.link());
    await first.command("view.snapshot");
    first.disconnect();
    const second = new PhoneClient(store, {}, factory(desktop));
    expect(await second.resume()).toBe(true);
    await tick();
    const r = await second.command("thread.read", { threadId: "th_1" });
    expect(r.ok).toBe(true);
  });

  it("F-RM-03 a code that never gets a welcome fails and stores nothing", async () => {
    const desktop = new FakeDesktop();
    desktop.replied = false;
    const store = new MemoryPairingStore();
    const client = new PhoneClient(store, {}, factory(desktop));
    const pairing = client.pair({ ...desktop.link(), s: b64url(new Uint8Array(32)) });
    // The desktop drops a hello with the wrong tag; closing ends the attempt.
    await tick();
    desktop.socket?.close(1000);
    await expect(pairing).rejects.toThrow(/expired/);
    expect(await store.load()).toBeNull();
  });

  it("F-RM-03 rekeying: the phone answers and switches when the desktop uses the next epoch", async () => {
    const desktop = new FakeDesktop();
    const store = new MemoryPairingStore();
    const client = new PhoneClient(store, {}, factory(desktop));
    await client.pair(desktop.link());
    const eph = generateKeyPair();
    desktop.replied = false;
    desktop.send({ t: "rekey", eph: b64url(eph.public) });
    await tick();
    const ack = desktop.received.at(-1) as { t: string; eph: string };
    expect(ack.t).toBe("rekey_ack");
    desktop.session = completeRekey(desktop.session!, eph.secret, fromB64url(ack.eph));
    desktop.send({ t: "changed" });
    await tick();
    expect((await store.load())?.session.current.epoch).toBe(1);
    desktop.replied = true;
    expect((await client.command("view.snapshot")).ok).toBe(true);
  });

  it("F-RM-03.4 a revoked pairing (bye, or the relay's 1008) wipes the phone's keys", async () => {
    const desktop = new FakeDesktop();
    const store = new MemoryPairingStore();
    const states: string[] = [];
    const client = new PhoneClient(store, { state: (s) => states.push(s) }, factory(desktop));
    await client.pair(desktop.link());
    desktop.send({ t: "bye", reason: "revoked" });
    await tick();
    expect(await store.load()).toBeNull();
    expect(client.connection).toBe("revoked");

    const desktop2 = new FakeDesktop();
    const store2 = new MemoryPairingStore();
    const client2 = new PhoneClient(store2, {}, factory(desktop2));
    await client2.pair(desktop2.link());
    desktop2.socket?.close(1008, "revoked");
    await tick();
    expect(await store2.load()).toBeNull();
    expect(client2.connection).toBe("revoked");
  });

  it("F-RM-03 the stored pairing round-trips through its JSON form", async () => {
    const desktop = new FakeDesktop();
    const store = new MemoryPairingStore();
    const client = new PhoneClient(store, {}, factory(desktop));
    const p = await client.pair(desktop.link());
    expect(deserializePairing(serializePairing(p))).toEqual(p);
  });
});
