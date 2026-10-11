// Where Vibezeo Remote keeps its pairing (F-RM-05, D-2010): the iOS Keychain
// or the Android Keystore through expo-secure-store, never plain storage.
// iOS items are readable only while the device is unlocked, on this device
// (no iCloud Keychain, no backups). Values are split into 1,800-character
// parts because some Android versions refuse larger Keystore values.
import { deserializePairing, serializePairing, type PairingStore, type StoredPairing } from "@tandem/remote/store";

/** The part of expo-secure-store this uses (a seam for tests). */
export type SecureKv = {
  getItemAsync(key: string, options?: object): Promise<string | null>;
  setItemAsync(key: string, value: string, options?: object): Promise<void>;
  deleteItemAsync(key: string, options?: object): Promise<void>;
};

export const PART = 1800;
const KEY = "vibezeo.pairing";

type Pointer = { gen: "a" | "b"; n: number };

/**
 * Writes go to the generation not in use, then one write flips the pointer,
 * so an interrupted save leaves the previous pairing readable.
 */
export class SecureStorePairingStore implements PairingStore {
  constructor(
    private readonly kv: SecureKv,
    private readonly options: object = {},
    private readonly key = KEY,
  ) {}

  private async pointer(): Promise<Pointer | null> {
    const raw = await this.kv.getItemAsync(`${this.key}.ptr`, this.options);
    const m = raw ? /^([ab]):(\d{1,2})$/.exec(raw) : null;
    if (!m) return null;
    const n = Number(m[2]);
    return n > 0 ? { gen: m[1] as "a" | "b", n } : null;
  }

  private part(gen: string, i: number): string {
    return `${this.key}.${gen}${i}`;
  }

  async load(): Promise<StoredPairing | null> {
    const ptr = await this.pointer();
    if (!ptr) return null;
    let text = "";
    for (let i = 0; i < ptr.n; i++) {
      const part = await this.kv.getItemAsync(this.part(ptr.gen, i), this.options);
      if (part === null) return null;
      text += part;
    }
    try {
      return deserializePairing(text);
    } catch {
      return null;
    }
  }

  async save(pairing: StoredPairing): Promise<void> {
    const text = serializePairing(pairing);
    const old = await this.pointer();
    const gen = old?.gen === "a" ? "b" : "a";
    const n = Math.max(1, Math.ceil(text.length / PART));
    for (let i = 0; i < n; i++) {
      await this.kv.setItemAsync(this.part(gen, i), text.slice(i * PART, (i + 1) * PART), this.options);
    }
    await this.kv.setItemAsync(`${this.key}.ptr`, `${gen}:${n}`, this.options);
    if (old) for (let i = 0; i < old.n; i++) await this.kv.deleteItemAsync(this.part(old.gen, i), this.options);
  }

  async clear(): Promise<void> {
    const ptr = await this.pointer();
    await this.kv.deleteItemAsync(`${this.key}.ptr`, this.options);
    if (ptr) for (let i = 0; i < ptr.n; i++) await this.kv.deleteItemAsync(this.part(ptr.gen, i), this.options);
  }
}
