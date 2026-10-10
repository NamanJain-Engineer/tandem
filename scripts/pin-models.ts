// Checks crates/tandem-voice/models/catalog.json against Hugging Face: each
// file's size and SHA-256 (the LFS record, or the downloaded bytes for small
// non-LFS files) at the pinned revision. Exits 1 on any difference, or when
// a value isn't pinned. `--write` updates the catalog in place instead.
// Usage: pnpm tsx scripts/pin-models.ts [--write]
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type PerFile<T> = T | Record<string, T>;
interface Model {
  id: string;
  url: string;
  files?: string[];
  sha256: PerFile<string>;
  bytes: PerFile<number>;
}

const root = new URL("..", import.meta.url).pathname;
const path = join(root, "crates/tandem-voice/models/catalog.json");
const catalog = JSON.parse(readFileSync(path, "utf8")) as { models: Model[] };
const write = process.argv.includes("--write");

async function lfs(repo: string, rev: string): Promise<Map<string, { size: number; oid?: string }>> {
  const res = await fetch(`https://huggingface.co/api/models/${repo}/tree/${rev}`);
  if (!res.ok) throw new Error(`${repo}@${rev}: HTTP ${res.status}`);
  const list = (await res.json()) as Array<{ path: string; size: number; lfs?: { oid: string } }>;
  return new Map(list.map((e) => [e.path, { size: e.size, oid: e.lfs?.oid }]));
}

async function sha(url: string): Promise<string> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return createHash("sha256").update(Buffer.from(await res.arrayBuffer())).digest("hex");
}

let bad = 0;
for (const m of catalog.models) {
  const match = /huggingface\.co\/([^/]+\/[^/]+)\/resolve\/([^/]+)\/(.*)$/.exec(m.url);
  if (!match) throw new Error(`${m.id}: not a Hugging Face resolve URL`);
  const [, repo, rev, rest] = match;
  const files = m.files ?? [rest];
  const tree = await lfs(repo, rev);
  for (const f of files) {
    const entry = tree.get(f);
    if (!entry) throw new Error(`${m.id}: ${f} isn't in ${repo}@${rev}`);
    const want = entry.oid ?? (await sha(m.files ? `${m.url}${f}` : m.url));
    const gotSha = typeof m.sha256 === "string" ? m.sha256 : m.sha256[f];
    const gotBytes = typeof m.bytes === "number" ? m.bytes : m.bytes[f];
    if (gotSha !== want || gotBytes !== entry.size) {
      console.error(`${m.id}/${f}: catalog ${gotSha} ${gotBytes}, upstream ${want} ${entry.size}`);
      bad += 1;
      if (write) {
        if (typeof m.sha256 === "string") m.sha256 = want;
        else m.sha256[f] = want;
        if (typeof m.bytes === "number") m.bytes = entry.size;
        else m.bytes[f] = entry.size;
      }
    } else {
      console.log(`${m.id}/${f}: ok`);
    }
  }
}
if (write && bad > 0) writeFileSync(path, JSON.stringify(catalog, null, 2) + "\n");
process.exit(bad > 0 && !write ? 1 : 0);
