// `pnpm tsx scripts/perf-summary.ts`: gathers the latest run of every perf
// suite (perf/results/m3, m5, m7 and the memory run in m15) into
// perf/results/m15.json (MILESTONES M15 task 7), with each suite's misses
// listed. The suites write their own files; this only collects them.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type Metric = { value: number; target: number; op: string; unit: string; pass: boolean };
type Run = { date: string; machine: Record<string, unknown>; metrics: Record<string, Metric> };

const dir = join(import.meta.dirname, "../perf/results");
const read = (f: string) => (existsSync(join(dir, f)) ? (JSON.parse(readFileSync(join(dir, f), "utf8")) as Record<string, unknown>) : {});

const sources: Array<[string, string, string]> = [
  ["terminals", "m3.json", "terminals"],
  ["startup", "m5.json", "startup"],
  ["threads", "m7.json", "threads"],
  ["memory", "m15.json", "memory"],
];
const out: Record<string, unknown> = { ...read("m15.json") };
const summary: Record<string, { measured: number; met: number; missed: string[] }> = {};
for (const [suite, file, key] of sources) {
  const body = read(file);
  const run = (body[key] ?? body) as Run;
  if (!run?.metrics) continue;
  out[suite] = run;
  const entries = Object.entries(run.metrics);
  summary[suite] = {
    measured: entries.length,
    met: entries.filter(([, m]) => m.pass).length,
    missed: entries.filter(([, m]) => !m.pass).map(([k, m]) => `${k} ${m.value} ${m.unit} (target ${m.op} ${m.target})`),
  };
}
out.summary = summary;
writeFileSync(join(dir, "m15.json"), `${JSON.stringify(out, null, 2)}\n`);
for (const [suite, s] of Object.entries(summary)) console.log(`${suite}: ${s.met}/${s.measured} met${s.missed.length ? `; missed ${s.missed.join("; ")}` : ""}`);
