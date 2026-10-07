// Fails when a waiver in security/waivers.toml has expired, or when an audit
// report (cargo audit / pnpm audit JSON) has a high or critical finding that
// isn't waived.
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
type Waiver = { id: string; reason: string; expires: string };

export function parseWaivers(toml: string): Waiver[] {
  const out: Waiver[] = [];
  let cur: Partial<Waiver> | null = null;
  for (const raw of toml.split("\n")) {
    const line = raw.replace(/#.*$/, "").trim();
    if (!line) continue;
    if (line === "[[waiver]]") {
      if (cur) out.push(cur as Waiver);
      cur = {};
      continue;
    }
    const m = line.match(/^(\w+)\s*=\s*"(.*)"$/);
    if (m && cur) (cur as Record<string, string>)[m[1] as string] = m[2] as string;
  }
  if (cur) out.push(cur as Waiver);
  return out;
}

const waivers = parseWaivers(readFileSync(join(root, "security/waivers.toml"), "utf8"));
const today = new Date().toISOString().slice(0, 10);
let failed = false;
for (const w of waivers) {
  if (!w.id || !w.expires || !w.reason) {
    console.error(`waiver missing id/reason/expires: ${JSON.stringify(w)}`);
    failed = true;
  } else if (w.expires < today) {
    console.error(`waiver ${w.id} expired on ${w.expires}`);
    failed = true;
  }
}
const waived = new Set(waivers.map((w) => w.id));

const arg = (name: string) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : undefined;
};
const cargoReport = arg("--cargo");
if (cargoReport && existsSync(cargoReport)) {
  const report = JSON.parse(readFileSync(cargoReport, "utf8")) as {
    vulnerabilities?: { list?: Array<{ advisory: { id: string; title: string } }> };
  };
  for (const v of report.vulnerabilities?.list ?? []) {
    if (!waived.has(v.advisory.id)) {
      console.error(`cargo: ${v.advisory.id} ${v.advisory.title}`);
      failed = true;
    }
  }
}
const pnpmReport = arg("--pnpm");
if (pnpmReport && existsSync(pnpmReport)) {
  const text = readFileSync(pnpmReport, "utf8").trim();
  const report = text
    ? (JSON.parse(text) as {
        advisories?: Record<string, { github_advisory_id?: string; severity: string; title: string }>;
      })
    : {};
  for (const a of Object.values(report.advisories ?? {})) {
    const id = a.github_advisory_id ?? a.title;
    if ((a.severity === "high" || a.severity === "critical") && !waived.has(id)) {
      console.error(`npm: ${id} (${a.severity}) ${a.title}`);
      failed = true;
    }
  }
}
if (failed) process.exit(1);
console.log(`waivers ok (${waivers.length})`);
