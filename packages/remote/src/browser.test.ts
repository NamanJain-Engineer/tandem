import "fake-indexeddb/auto";
import { afterEach, describe, expect, it, vi } from "vitest";
import { IndexedDbPairingStore } from "./browser";
import { b64url, generateKeyPair, newSession } from "./protocol";
import type { StoredPairing } from "./store";

function pairing(): StoredPairing {
  const keys = generateKeyPair();
  return {
    link: { v: 1, pid: "pr_lanstore", dpk: b64url(keys.public), relay: "ws://192.168.1.20:47800/v1/relay/pr_lanstore", ptok: "tok" },
    phoneSecret: keys.secret,
    session: newSession("pr_lanstore", "phone", new Uint8Array(32).fill(3)),
    desktop: "Desk",
    grants: { chat: true, terminal: false, approvals: false, routines: false },
    pairedAt: 1,
  };
}

describe("IndexedDbPairingStore", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("F-RM-04.5 seals the pairing without WebCrypto's subtle on a LAN page (not a secure context)", async () => {
    vi.stubGlobal("crypto", { getRandomValues: globalThis.crypto.getRandomValues.bind(globalThis.crypto) });
    const store = new IndexedDbPairingStore();
    await store.save(pairing());
    const loaded = await store.load();
    expect(loaded?.link.relay).toBe("ws://192.168.1.20:47800/v1/relay/pr_lanstore");
    expect(loaded?.grants.chat).toBe(true);
    // What sits in IndexedDB is ciphertext, never the session keys.
    const raw = await new Promise<unknown>((resolve) => {
      const open = indexedDB.open("vibezeo-remote", 1);
      open.onsuccess = () => {
        const get = open.result.transaction("kv").objectStore("kv").get("pairing");
        get.onsuccess = () => resolve(get.result);
      };
    });
    expect(JSON.stringify(raw)).not.toContain("pr_lanstore");
    await store.clear();
    expect(await store.load()).toBeNull();
  });
});
