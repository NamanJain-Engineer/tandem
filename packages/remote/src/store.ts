// What the phone remembers about its pairing (F-RM-03, ARCHITECTURE §13.3)
// and where it keeps it. The browser's store is in `browser.ts` (IndexedDB,
// D-1909); the Vibezeo Remote app keeps it in the iOS Keychain or the
// Android Keystore (`apps/mobile`, D-2010).
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
