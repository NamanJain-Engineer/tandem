import { describe, expect, it } from "vitest";
import { b64url, generateKeyPair, newSession } from "@tandem/remote/protocol";
import type { StoredPairing } from "@tandem/remote/store";
import { PART, SecureStorePairingStore, type SecureKv } from "./secureStore";

class FakeKv implements SecureKv {
  readonly map = new Map<string, string>();
  writes = 0;
  failAfter = Infinity;
  async getItemAsync(k: string) {
    return this.map.get(k) ?? null;
  }
  async setItemAsync(k: string, v: string) {
    if (this.writes++ >= this.failAfter) throw new Error("disk full");
    if (v.length > 2048) throw new Error("value too large for the keystore");
    this.map.set(k, v);
  }
  async deleteItemAsync(k: string) {
    this.map.delete(k);
  }
}

function pairing(desktop = "Desk"): StoredPairing {
  const keys = generateKeyPair();
  return {
    link: { v: 1, pid: "pr_securestore", dpk: b64url(keys.public), relay: "wss://api.vibezeo.com/v1/relay/pr_securestore", ptok: "x".repeat(43) },
    phoneSecret: keys.secret,
    session: newSession("pr_securestore", "phone", new Uint8Array(32).fill(9)),
    desktop,
    grants: { chat: true, terminal: true, approvals: false, routines: false },
    pairedAt: 1,
  };
}

describe("F-RM-05.5 the pairing lives in the Keychain / Keystore", () => {
  it("round-trips, split into parts the keystore accepts", async () => {
    const kv = new FakeKv();
    const store = new SecureStorePairingStore(kv);
    const p = pairing("A computer with a long name ".repeat(100));
    await store.save(p);
    const loaded = await store.load();
    expect(loaded?.desktop).toBe(p.desktop);
    expect(loaded?.session.txCtr).toBe(p.session.txCtr);
    expect(loaded?.phoneSecret).toEqual(p.phoneSecret);
    expect([...kv.map.values()].every((v) => v.length <= PART)).toBe(true);
    expect([...kv.map.keys()].filter((k) => !k.endsWith(".ptr")).length).toBeGreaterThan(1);
  });

  it("an interrupted save leaves the previous pairing readable", async () => {
    const kv = new FakeKv();
    const store = new SecureStorePairingStore(kv);
    await store.save(pairing("First"));
    kv.writes = 0;
    kv.failAfter = 1;
    await expect(store.save(pairing("Second ".repeat(400)))).rejects.toThrow("disk full");
    expect((await store.load())?.desktop).toBe("First");
  });

  it("clear forgets everything; nothing stored reads as unpaired", async () => {
    const kv = new FakeKv();
    const store = new SecureStorePairingStore(kv);
    expect(await store.load()).toBeNull();
    await store.save(pairing());
    await store.save(pairing("Again"));
    await store.clear();
    expect(kv.map.size).toBe(0);
    expect(await store.load()).toBeNull();
  });
});
