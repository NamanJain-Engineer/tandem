#!/usr/bin/env node
// What Claude Code's prompt drops from a message before it reads it, as a
// fixture the defang tests check `untrusted::removable` against (D-3030).
//
// The binary embeds its JavaScript. This finds the submit-time stripper's
// module by what it counts (`{tags:0,bidi:0,zeroWidth:0,selectors:0,other:0}`)
// rather than by its minified name, replaces the module's imports with inert
// stubs, evaluates it in a `vm` context and calls the stripper on every code
// point in a few contexts. The binary itself is never run. The stubbed imports
// only matter for text that isn't well formed (lone surrogates), which isn't
// evaluated.
//
//   node scripts/agent/claude-code-strip.mjs [path/to/claude] [out.json]
//
// Re-run it when the Claude Code version the fixture names moves
// (docs/OPERATING.md §5).
import { execFileSync } from "node:child_process";
import { readFileSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
// An empty argument takes the default too: `"" out.json` reads the installed one.
const binPath = realpathSync(
  process.argv[2] || execFileSync("sh", ["-c", "command -v claude"], { encoding: "utf8" }).trim(),
);
const outPath = process.argv[3] || join(root, "crates/tandem-core/tests/fixtures/claude-code-strip.json");

const bin = readFileSync(binPath);
const latin = bin.toString("latin1");
const MARKER = "{tags:0,bidi:0,zeroWidth:0,selectors:0,other:0}";

function findStripper() {
  for (let at = latin.indexOf(MARKER); at !== -1; at = latin.indexOf(MARKER, at + 1)) {
    const fn = /function ([\w$]+)\([\w$]+\)\{let [\w$]+=$/.exec(latin.slice(at - 80, at));
    if (!fn) continue;
    const start = latin.lastIndexOf("// @bun", at);
    const end = latin.indexOf("\u0000", at);
    if (start === -1 || end === -1) continue;
    return { name: fn[1], source: bin.subarray(start, end).toString("utf8") };
  }
  throw new Error(`no stripper (a function starting with ${MARKER}) in ${binPath}`);
}

/** An imported helper: callable (it returns its first argument), any property
 * is another stub, and it reads as "" where the module builds a pattern. */
function stub() {
  const f = (x) => x;
  return new Proxy(f, {
    get(target, prop) {
      if (prop === Symbol.toPrimitive || prop === "toString" || prop === "valueOf") return () => "";
      if (prop in target) return target[prop];
      return stub();
    },
  });
}

function load({ name, source }) {
  const imported = [];
  const local = (spec) =>
    spec
      .trim()
      .split(/\s+as\s+/)
      .pop();
  let src = source
    .replace(/import\s*\{([^}]*)\}\s*from\s*"[^"]*";?/g, (_, names) => {
      for (const n of names.split(",")) if (local(n)) imported.push(local(n));
      return "";
    })
    .replace(/import\s*\*\s*as\s+([\w$]+)\s+from\s*"[^"]*";?/g, (_, n) => (imported.push(n), ""))
    .replace(/import\s+([\w$]+)\s+from\s*"[^"]*";?/g, (_, n) => (imported.push(n), ""))
    .replace(/export\s*\{[^}]*\};?/g, "");
  const sandbox = { process: { pid: 1, env: {}, platform: "linux" }, console };
  for (const n of imported) sandbox[n] = stub();
  vm.createContext(sandbox);
  vm.runInContext(`${src}\n;globalThis.__strip = ${name};`, sandbox, { timeout: 60_000 });
  return (text) => sandbox.__strip(text).text;
}

function ranges(cps) {
  const out = [];
  for (const cp of cps) {
    const last = out.at(-1);
    if (last && last[1] === cp - 1) last[1] = cp;
    else out.push([cp, cp]);
  }
  return out;
}

const stripper = findStripper();
const version = /\/\/ Version: (\S+)/.exec(stripper.source)?.[1];
if (!version) throw new Error("the stripper's module names no version");
const strip = load(stripper);

// Alone, at either end and between letters, digits or spaces: a code point
// counts as dropped if any context drops it.
const contexts = [
  ["a", "b"],
  ["", ""],
  ["a", ""],
  ["", "b"],
  ["1", "2"],
  [" ", " "],
];
const removed = [];
const newline = [];
const otherwise = [];
for (let cp = 0; cp <= 0x10ffff; cp++) {
  if (cp >= 0xd800 && cp <= 0xdfff) continue;
  const c = String.fromCodePoint(cp);
  let gone = false;
  let broken = false;
  let changed = false;
  for (const [l, r] of contexts) {
    const out = strip(l + c + r);
    if (out === l + r) gone = true;
    else if (out === `${l}\n${r}` && cp !== 0x0a) broken = true;
    else if (out !== l + c + r) changed = true;
  }
  if (gone) removed.push(cp);
  else if (broken) newline.push(cp);
  if (changed && !gone && !broken) otherwise.push(cp);
}
if (otherwise.length > 0) {
  throw new Error(`the stripper changes ${otherwise.length} code points some other way; read it again`);
}

const fixture = {
  about:
    "What Claude Code's prompt drops from a message before it reads it, and what it reads as a line break: its submit-time stripper, " +
    "run over every code point by scripts/agent/claude-code-strip.mjs from the installed binary (the binary isn't run). " +
    "untrusted::removable, and the app's, must cover `removed` (D-3030). Ranges are inclusive code points.",
  version,
  removed: ranges(removed),
  newline: ranges(newline),
};
writeFileSync(
  outPath,
  `${JSON.stringify(fixture, null, 2).replace(/\[\s+(\d+),\s+(\d+)\s+\]/g, "[$1, $2]")}\n`,
);
console.log(
  `Claude Code ${version} (${stripper.name}): drops ${removed.length} code points in ${fixture.removed.length} ranges, ` +
    `reads ${newline.length} as a line break → ${outPath}`,
);
