// `pnpm keys:dev`: generates Ed25519 dev keys into .dev-keys/keys.json (git-
// ignored) and copies the public keys to the desktop's resources/keys.json.
import { createPrivateKey, createPublicKey } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { generateKeyPairSync } from "node:crypto";

const root = new URL("..", import.meta.url).pathname;
const devDir = join(root, ".dev-keys");
const devFile = join(devDir, "keys.json");

function pem(): string {
  return generateKeyPairSync("ed25519").privateKey.export({ format: "pem", type: "pkcs8" }).toString();
}

function rawPub(privatePem: string): string {
  const der = createPublicKey(createPrivateKey(privatePem)).export({ format: "der", type: "spki" });
  return Buffer.from(der.subarray(der.length - 32)).toString("base64");
}

let keys: Record<string, string>;
if (existsSync(devFile) && !process.argv.includes("--force")) {
  keys = JSON.parse(readFileSync(devFile, "utf8")) as Record<string, string>;
} else {
  keys = { "ak-1": pem(), "ek-1": pem(), "ck-1": pem() };
  mkdirSync(devDir, { recursive: true, mode: 0o700 });
  writeFileSync(devFile, JSON.stringify(keys, null, 2) + "\n", { mode: 0o600 });
}

const pub = (prefix: string) =>
  Object.fromEntries(
    Object.entries(keys)
      .filter(([kid]) => kid.startsWith(prefix))
      .map(([kid, p]) => [kid, rawPub(p)]),
  );
const resources = join(root, "apps/desktop/src-tauri/resources");
mkdirSync(resources, { recursive: true });
writeFileSync(
  join(resources, "keys.json"),
  JSON.stringify({ entitlement: pub("ek-"), config: pub("ck-") }, null, 2) + "\n",
);
console.log(
  `dev keys: ${Object.keys(keys).join(", ")} → .dev-keys/keys.json; public keys → resources/keys.json`,
);
