// The browser's pairing store (F-RM-03, D-1909): IndexedDB, with the pairing
// sealed by a non-extractable AES-GCM key that also lives in IndexedDB, so
// script on this origin can use it but can't read it out. X25519 and
// XChaCha20 keys aren't WebCrypto key types everywhere, so they can't be
// non-extractable themselves.
//
// The desktop serves the same web app over plain HTTP on the local network
// (LAN mode, D-2004). Pages there aren't a secure context and have no
// `crypto.subtle`, so the pairing is sealed with XChaCha20-Poly1305 under a
// random key kept beside it instead: it protects against nothing that the
// AES-GCM key doesn't, and it's documented as the LAN residual risk.
import { xchacha20poly1305 } from "@noble/ciphers/chacha.js";
import { randomBytes } from "@noble/ciphers/utils.js";
import { deserializePairing, serializePairing, type PairingStore, type StoredPairing } from "./store";

const DB = "vibezeo-remote";
const STORE = "kv";

function request<T>(req: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error ?? new Error("IndexedDB request failed"));
  });
}

function openDb(): Promise<IDBDatabase> {
  const req = indexedDB.open(DB, 1);
  req.onupgradeneeded = () => req.result.createObjectStore(STORE);
  return request(req);
}

async function kv<T>(mode: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await openDb();
  try {
    return await request(fn(db.transaction(STORE, mode).objectStore(STORE)));
  } finally {
    db.close();
  }
}

/** Whether this page has WebCrypto's `subtle` (secure contexts only). */
export function hasSubtle(): boolean {
  return typeof globalThis.crypto !== "undefined" && typeof globalThis.crypto.subtle !== "undefined";
}

type Sealed = { iv: Uint8Array; ct: ArrayBuffer } | { alg: "xchacha"; nonce: Uint8Array; ct: Uint8Array };

/** The browser's store: the pairing sealed with a non-extractable AES-GCM key (or, without WebCrypto, see above). */
export class IndexedDbPairingStore implements PairingStore {
  private async rawKey(): Promise<Uint8Array> {
    const existing = await kv<Uint8Array | undefined>("readonly", (s) => s.get("wrap-raw"));
    if (existing) return existing;
    const key = randomBytes(32);
    await kv("readwrite", (s) => s.put(key, "wrap-raw"));
    return key;
  }

  private async wrapKey(): Promise<CryptoKey> {
    const existing = await kv<CryptoKey | undefined>("readonly", (s) => s.get("wrap"));
    if (existing) return existing;
    const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
    await kv("readwrite", (s) => s.put(key, "wrap"));
    return key;
  }

  async load(): Promise<StoredPairing | null> {
    const sealed = await kv<Sealed | undefined>("readonly", (s) => s.get("pairing"));
    if (!sealed) return null;
    try {
      if ("alg" in sealed) {
        const plain = xchacha20poly1305(await this.rawKey(), sealed.nonce).decrypt(sealed.ct);
        return deserializePairing(new TextDecoder().decode(plain));
      }
      const key = await this.wrapKey();
      const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: sealed.iv }, key, sealed.ct);
      return deserializePairing(new TextDecoder().decode(plain));
    } catch {
      return null;
    }
  }

  async save(pairing: StoredPairing): Promise<void> {
    if (!hasSubtle()) {
      const nonce = randomBytes(24);
      const ct = xchacha20poly1305(await this.rawKey(), nonce).encrypt(
        new TextEncoder().encode(serializePairing(pairing)),
      );
      await kv("readwrite", (s) => s.put({ alg: "xchacha", nonce, ct }, "pairing"));
      return;
    }
    const key = await this.wrapKey();
    const iv = crypto.getRandomValues(new Uint8Array(12));
    const ct = await crypto.subtle.encrypt(
      { name: "AES-GCM", iv },
      key,
      new TextEncoder().encode(serializePairing(pairing)),
    );
    await kv("readwrite", (s) => s.put({ iv, ct }, "pairing"));
  }

  async clear(): Promise<void> {
    await kv("readwrite", (s) => s.delete("pairing"));
  }
}
