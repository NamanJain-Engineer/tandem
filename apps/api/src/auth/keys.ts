// Ed25519 signing keys. `kid` prefixes: ak-* access JWTs, ek-* entitlement
// tokens, ck-* remote config (ARCHITECTURE §14.3, §14.4, §14.10).
import { createPrivateKey, createPublicKey, generateKeyPairSync, type KeyObject } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

export type KeyKind = "ak" | "ek" | "ck";
export type SigningKey = { kid: string; privateKey: KeyObject; publicKey: KeyObject };
export type KeySet = { access: SigningKey[]; entitlement: SigningKey[]; config: SigningKey[] };

function fromPem(kid: string, pem: string): SigningKey {
  const privateKey = createPrivateKey(pem);
  return { kid, privateKey, publicKey: createPublicKey(privateKey) };
}

function generate(kid: string): SigningKey {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  return { kid, privateKey, publicKey };
}

/** Raw 32-byte public key, base64 — the format bundled in the desktop's keys.json. */
export function rawPublicKey(key: SigningKey): string {
  const der = key.publicKey.export({ format: "der", type: "spki" });
  return Buffer.from(der.subarray(der.length - 32)).toString("base64");
}

function group(entries: Record<string, string>): KeySet {
  const set: KeySet = { access: [], entitlement: [], config: [] };
  for (const [kid, pem] of Object.entries(entries)) {
    const key = fromPem(kid, pem);
    if (kid.startsWith("ak-")) set.access.push(key);
    else if (kid.startsWith("ek-")) set.entitlement.push(key);
    else if (kid.startsWith("ck-")) set.config.push(key);
  }
  return set;
}

/**
 * Production: `JWT_SIGNING_KEYS` (JSON `{kid: pkcs8 PEM}` for the ak and ek keys) and
 * `CONFIG_SIGNING_KEY` (JSON `{kid: PEM}` for the ck key). Dev: `.dev-keys/keys.json`
 * written by `pnpm keys:dev`. Tests: fresh keys per process.
 */
export function loadKeys(opts: {
  jwtKeys?: string;
  configKey?: string;
  repoRoot?: string;
  allowGenerate: boolean;
}): KeySet {
  const entries: Record<string, string> = {};
  if (opts.jwtKeys) Object.assign(entries, JSON.parse(opts.jwtKeys) as Record<string, string>);
  if (opts.configKey) Object.assign(entries, JSON.parse(opts.configKey) as Record<string, string>);
  const devFile = join(opts.repoRoot ?? process.cwd(), ".dev-keys/keys.json");
  if (Object.keys(entries).length === 0 && existsSync(devFile)) {
    Object.assign(entries, JSON.parse(readFileSync(devFile, "utf8")) as Record<string, string>);
  }
  const set = group(entries);
  if (opts.allowGenerate) {
    if (set.access.length === 0) set.access.push(generate("ak-1"));
    if (set.entitlement.length === 0) set.entitlement.push(generate("ek-1"));
    if (set.config.length === 0) set.config.push(generate("ck-1"));
  }
  if (set.access.length === 0 || set.entitlement.length === 0 || set.config.length === 0) {
    throw new Error(
      "signing keys missing: set JWT_SIGNING_KEYS and CONFIG_SIGNING_KEY, or run `pnpm keys:dev`",
    );
  }
  return set;
}

/** The newest key of a kind signs; every listed key verifies (rotation). */
export function currentKey(keys: SigningKey[]): SigningKey {
  const key = keys[keys.length - 1];
  if (!key) throw new Error("no signing key");
  return key;
}

export function newPem(): string {
  const { privateKey } = generateKeyPairSync("ed25519");
  return privateKey.export({ format: "pem", type: "pkcs8" }).toString();
}
