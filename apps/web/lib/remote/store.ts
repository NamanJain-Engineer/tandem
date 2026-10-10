// Where the phone keeps its pairing (F-RM-03, ARCHITECTURE §13.3). In the
// browser it's IndexedDB, with the pairing sealed by a non-extractable
// AES-GCM key that also lives in IndexedDB: script on this origin can use
// it but can't read it out. X25519 and XChaCha20 keys aren't WebCrypto key
// types everywhere, so they can't be non-extractable themselves (D-1009).
import { b64url, fromB64url, type EpochKeys, type PairingLink, type SessionState } from "./protocol";
import type { PhoneGrants } from "./messages";

/** Everything the phone remembers about its pairing. */
export type StoredPairing = {
  link: Omit<PairingLink, "s">;
  phoneSecret: Uint8Array;
  session: SessionState;
  desktop: string;
  grants: PhoneGrants;
  pairedAt: number;
};

export interface PairingStore {
  load(): Promise<StoredPairing | null>;
  save(pairing: StoredPairing): Promise<void>;
  clear(): Promise<void>;
}

type EpochJson = { epoch: number; secret: string; d2p: string; p2d: string };

function epochToJson(e: EpochKeys): EpochJson {
  return { epoch: e.epoch, secret: b64url(e.secret), d2p: b64url(e.d2p), p2d: b64url(e.p2d) };
}

function epochFromJson(e: EpochJson): EpochKeys {
  return { epoch: e.epoch, secret: fromB64url(e.secret), d2p: fromB64url(e.d2p), p2d: fromB64url(e.p2d) };
}

/** A JSON form of the pairing (bytes as base64url, counters as strings). */
export function serializePairing(p: StoredPairing): string {
  const s = p.session;
  return JSON.stringify({
    link: p.link,
    phoneSecret: b64url(p.phoneSecret),
    desktop: p.desktop,
    grants: p.grants,
    pairedAt: p.pairedAt,
    session: {
      pid: s.pid,
      role: s.role,
      current: epochToJson(s.current),
      next: s.next ? epochToJson(s.next) : null,
      txOnNext: s.txOnNext,
      txCtr: s.txCtr.toString(),
      rxCtr: s.rxCtr.toString(),
    },
  });
}

export function deserializePairing(text: string): StoredPairing {
  const raw = JSON.parse(text) as {
    link: StoredPairing["link"];
    phoneSecret: string;
    desktop: string;
    grants: PhoneGrants;
    pairedAt: number;
    session: {
      pid: string;
      role: SessionState["role"];
      current: EpochJson;
      next: EpochJson | null;
      txOnNext: boolean;
      txCtr: string;
      rxCtr: string;
    };
  };
  return {
    link: raw.link,
    phoneSecret: fromB64url(raw.phoneSecret),
    desktop: raw.desktop,
    grants: raw.grants,
    pairedAt: raw.pairedAt,
    session: {
      pid: raw.session.pid,
      role: raw.session.role,
      current: epochFromJson(raw.session.current),
      next: raw.session.next ? epochFromJson(raw.session.next) : undefined,
      txOnNext: raw.session.txOnNext,
      txCtr: BigInt(raw.session.txCtr),
      rxCtr: BigInt(raw.session.rxCtr),
    },
  };
}

/** For tests and the scripted phones in E2E runs. */
export class MemoryPairingStore implements PairingStore {
  private value: string | null = null;

  async load(): Promise<StoredPairing | null> {
    return this.value ? deserializePairing(this.value) : null;
  }

  async save(pairing: StoredPairing): Promise<void> {
    this.value = serializePairing(pairing);
  }

  async clear(): Promise<void> {
    this.value = null;
  }
}

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

/** The browser's store: the pairing sealed with a non-extractable AES-GCM key. */
export class IndexedDbPairingStore implements PairingStore {
  private async wrapKey(): Promise<CryptoKey> {
    const existing = await kv<CryptoKey | undefined>("readonly", (s) => s.get("wrap"));
    if (existing) return existing;
    const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
    await kv("readwrite", (s) => s.put(key, "wrap"));
    return key;
  }

  async load(): Promise<StoredPairing | null> {
    const sealed = await kv<{ iv: Uint8Array; ct: ArrayBuffer } | undefined>("readonly", (s) => s.get("pairing"));
    if (!sealed) return null;
    try {
      const key = await this.wrapKey();
      const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: sealed.iv }, key, sealed.ct);
      return deserializePairing(new TextDecoder().decode(plain));
    } catch {
      return null;
    }
  }

  async save(pairing: StoredPairing): Promise<void> {
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
