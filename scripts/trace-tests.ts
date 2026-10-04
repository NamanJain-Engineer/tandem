// Lists acceptance criteria of a milestone's owned features that have no test
// named after them (MILESTONES §0 "Milestone close").
//   Rust test names: f_cd_03_4_close_kills_tree
//   TS test names:   "F-CD-03.4 close kills tree"
//   Manual checks:   a BUILD_LOG line containing "[manual] F-CD-03.4"
// Usage: tsx scripts/trace-tests.ts --milestone M3 [--fail-on-missing]
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const args = process.argv.slice(2);
const milestone = args[args.indexOf("--milestone") + 1];
const failOnMissing = args.includes("--fail-on-missing");
if (!milestone || !/^M\d+$/.test(milestone)) {
  console.error("usage: tsx scripts/trace-tests.ts --milestone M<n> [--fail-on-missing]");
  process.exit(2);
}

export type Criterion = { id: string; n: number; manual: boolean; text: string };

/** Acceptance criteria per feature from PRODUCT_SPEC. */
export function parseCriteria(spec: string): Map<string, Criterion[]> {
  const out = new Map<string, Criterion[]>();
  const sections = spec.split(/^#### /m).slice(1);
  for (const section of sections) {
    const id = section.match(/^(F-[A-Z]{2}-\d{2})/)?.[1];
    if (!id) continue;
    const acc = section.split(/\*\*Acceptance:\*\*/)[1];
    if (!acc) continue;
    const list: Criterion[] = [];
    for (const line of acc.split("\n")) {
      const m = line.match(/^(\d+)\.\s+(.*)$/);
      if (m) list.push({ id, n: Number(m[1]), manual: /\[manual\]/i.test(m[2] ?? ""), text: m[2] ?? "" });
      else if (/^#{2,4} /.test(line)) break;
    }
    out.set(id, list);
  }
  return out;
}

/** IDs on the milestone's **Owns:** line(s). */
export function ownedBy(milestones: string, m: string): string[] {
  const block = milestones.split(/^## /m).find((b) => b.startsWith(`${m} ·`));
  if (!block) return [];
  const owns =
    block.split(/^\*\*Owns:?\*\*/m)[1]?.split(/^\*\*(Early|Completes|Extends|Prerequisites)/m)[0] ?? "";
  return [...new Set(owns.match(/F-[A-Z]{2}-\d{2}/g) ?? [])];
}

function walk(dir: string, acc: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (["node_modules", "target", ".git", "dist", ".next", "build-kit", "generated"].includes(name))
      continue;
    const p = join(dir, name);
    const s = statSync(p);
    if (s.isDirectory()) walk(p, acc);
    else if (/\.(rs|ts|tsx)$/.test(name)) acc.push(p);
  }
  return acc;
}

const spec = readFileSync(join(root, "build-kit/PRODUCT_SPEC.md"), "utf8");
const plan = readFileSync(join(root, "build-kit/MILESTONES.md"), "utf8");
const buildLog = readFileSync(join(root, "BUILD_LOG.md"), "utf8");
const criteria = parseCriteria(spec);
const sources = walk(root).map((f) => ({ f, text: readFileSync(f, "utf8") }));

let missing = 0;
let covered = 0;
for (const id of ownedBy(plan, milestone)) {
  for (const c of criteria.get(id) ?? []) {
    const rust = new RegExp(`fn ${id.toLowerCase().replace(/-/g, "_")}_${c.n}_\\w+`);
    const ts = new RegExp(`["'\`]${id}\\.${c.n} `);
    const hit = sources.find((s) => rust.test(s.text) || ts.test(s.text));
    const manual = buildLog.includes(`[manual] ${id}.${c.n}`);
    if (hit || manual) {
      covered++;
      continue;
    }
    missing++;
    console.log(`missing ${id}.${c.n}${c.manual ? " [manual]" : ""}: ${c.text.slice(0, 100)}`);
  }
}
console.log(`${milestone}: ${covered} criteria covered, ${missing} missing`);
void relative;
if (failOnMissing && missing > 0) process.exit(1);
