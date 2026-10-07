// Minisign keys and signatures in the encoding Tauri's updater reads
// (F-SH-06): the public key and each signature are base64 of the minisign
// file text. Signatures are prehashed ("ED": Ed25519 over BLAKE2b-512) and
// carry `version:<v>` in the trusted comment, which the updater checks
// against the version the feed announces.
import { createHash, createPrivateKey, createPublicKey, sign } from "node:crypto";

function rawPublicKey(privatePem: string): Buffer {
  const der = createPublicKey(createPrivateKey(privatePem)).export({ format: "der", type: "spki" });
  return Buffer.from(der.subarray(der.length - 32));
}

/** A stable 8-byte key id: the first bytes of the public key's hash. */
function keyId(privatePem: string): Buffer {
  return createHash("sha256").update(rawPublicKey(privatePem)).digest().subarray(0, 8);
}

/** `TAURI_SIGNING_PUBLIC_KEY`-style public key for `privatePem` (Ed25519). */
export function publicKey(privatePem: string): string {
  const id = keyId(privatePem);
  const bin = Buffer.concat([Buffer.from("Ed"), id, rawPublicKey(privatePem)]);
  const text = `untrusted comment: minisign public key: ${Buffer.from(id).reverse().toString("hex").toUpperCase()}\n${bin.toString("base64")}\n`;
  return Buffer.from(text).toString("base64");
}

/** A `.sig` for `data`, as the updater feed's `signature` field. */
export function signature(
  data: Buffer,
  privatePem: string,
  version: string,
  file = "tandem.AppImage",
): string {
  const key = createPrivateKey(privatePem);
  const digest = createHash("blake2b512").update(data).digest();
  const sig = sign(null, digest, key);
  const trusted = `timestamp:${Math.floor(Date.now() / 1000)}\tfile:${file}\tversion:${version}`;
  const global = sign(null, Buffer.concat([sig, Buffer.from(trusted)]), key);
  const line1 = Buffer.concat([Buffer.from("ED"), keyId(privatePem), sig]).toString("base64");
  const text = `untrusted comment: signature from tandem dev key\n${line1}\ntrusted comment: ${trusted}\n${global.toString("base64")}\n`;
  return Buffer.from(text).toString("base64");
}
