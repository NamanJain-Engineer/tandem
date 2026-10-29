#!/usr/bin/env node
// What Claude Code's prompt drops from a message before it reads it, as a
// fixture the defang tests check `untrusted::removable` against (D-3030).
//
// The binary embeds its JavaScript. This finds the submit-time stripper's
// module by what it counts (`{tags:0,bidi:0,zeroWidth:0,selectors:0,other:0}`)
// rather than by its minified name, and evaluates that module alone in a `vm`
// context that holds nothing of this process: the sandbox has no prototype,
// so no constructor leads back here, and the module's imports become inert
// stubs made inside the context. The sweep over every code point runs there
// too, under a timeout, and only its result, a string, comes out. The binary
// itself is never run. The stubbed imports only matter for text that isn't
// well formed (lone surrogates), which isn't evaluated.
//
//   node scripts/agent/claude-code-strip.mjs [path/to/claude] [out.json]
//
// Without a path it reads the first `claude` on PATH, found without a shell;
// pass the path to be sure which binary it reads. The fixture records the
// binary's SHA-256. Re-run it when the Claude Code version the fixture names
// moves (docs/OPERATING.md §5).
import { createHash } from "node:crypto";
import { accessSync, constants, readFileSync, realpathSync, statSync, writeFileSync } from "node:fs";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");

/** The first executable `claude` on PATH. */
function onPath() {
  for (const dir of (process.env.PATH ?? "").split(delimiter)) {
    if (!dir) continue;
    const candidate = join(dir, "claude");
    try {
      if (statSync(candidate).isFile()) {
        accessSync(candidate, constants.X_OK);
        return candidate;
      }
    } catch {
      // Not in this folder.
    }
  }
  throw new Error("no claude on PATH: pass the binary's path");
}

// An empty argument takes the default too: `"" out.json` reads the one on PATH.
const binPath = realpathSync(process.argv[2] || onPath());
const outPath = process.argv[3] || join(root, "crates/tandem-core/tests/fixtures/claude-code-strip.json");

const bin = readFileSync(binPath);
const sha256 = createHash("sha256").update(bin).digest("hex");
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

/** The module without its imports and exports, and the local names its imports had. */
function prepared(source) {
  const imported = [];
  const local = (spec) =>
    spec
      .trim()
      .split(/\s+as\s+/)
      .pop();
  const src = source
    .replace(/import\s*\{([^}]*)\}\s*from\s*"[^"]*";?/g, (_, names) => {
      for (const n of names.split(",")) if (local(n)) imported.push(local(n));
      return "";
    })
    .replace(/import\s*\*\s*as\s+([\w$]+)\s+from\s*"[^"]*";?/g, (_, n) => (imported.push(n), ""))
    .replace(/import\s+([\w$]+)\s+from\s*"[^"]*";?/g, (_, n) => (imported.push(n), ""))
    .replace(/export\s*\{[^}]*\};?/g, "");
  return { src, imported };
}

// Run inside the context. A stub is callable (it returns its first argument),
// any property is another stub, and it reads as "" where the module builds a
// pattern.
const SETUP = `
globalThis.process = { pid: 1, env: {}, platform: "linux" };
globalThis.__stub = function stub() {
  const f = (x) => x;
  return new Proxy(f, {
    get(target, prop) {
      if (prop === Symbol.toPrimitive || prop === "toString" || prop === "valueOf") return () => "";
      if (prop in target) return target[prop];
      return stub();
    },
  });
};`;

// Run inside the context: alone, at either end and between letters, digits or
// spaces, a code point counts as dropped if any context drops it.
const SWEEP = `(() => {
  const strip = (text) => __strip(text).text;
  const contexts = [["a", "b"], ["", ""], ["a", ""], ["", "b"], ["1", "2"], [" ", " "]];
  const removed = [];
  const newline = [];
  let otherwise = 0;
  for (let cp = 0; cp <= 0x10ffff; cp++) {
    if (cp >= 0xd800 && cp <= 0xdfff) continue;
    const c = String.fromCodePoint(cp);
    let gone = false;
    let broken = false;
    let changed = false;
    for (const [l, r] of contexts) {
      const out = strip(l + c + r);
      if (out === l + r) gone = true;
      else if (out === l + "\\n" + r && cp !== 0x0a) broken = true;
      else if (out !== l + c + r) changed = true;
    }
    if (gone) removed.push(cp);
    else if (broken) newline.push(cp);
    if (changed && !gone && !broken) otherwise += 1;
  }
  return JSON.stringify({ removed, newline, otherwise });
})()`;

function sweep({ name, source }) {
  const { src, imported } = prepared(source);
  const context = vm.createContext(Object.create(null));
  vm.runInContext(SETUP, context, { timeout: 5_000 });
  const stubs = imported.map((n) => `globalThis[${JSON.stringify(n)}] = __stub();`).join("\n");
  vm.runInContext(stubs, context, { timeout: 5_000 });
  vm.runInContext(`${src}\n;globalThis.__strip = ${name};`, context, { timeout: 60_000 });
  const out = vm.runInContext(SWEEP, context, { timeout: 600_000 });
  if (typeof out !== "string") throw new Error("the sweep gave no result");
  return JSON.parse(out);
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
const { removed, newline, otherwise } = sweep(stripper);
if (otherwise > 0) {
  throw new Error(`the stripper changes ${otherwise} code points some other way; read it again`);
}

const fixture = {
  about:
    "What Claude Code's prompt drops from a message before it reads it, and what it reads as a line break: its submit-time stripper, " +
    "run over every code point by scripts/agent/claude-code-strip.mjs from the binary whose SHA-256 is `sha256` (the binary isn't run). " +
    "untrusted::removable, and the app's, must cover `removed` (D-3030). Ranges are inclusive code points.",
  version,
  sha256,
  removed: ranges(removed),
  newline: ranges(newline),
};
writeFileSync(
  outPath,
  `${JSON.stringify(fixture, null, 2).replace(/\[\s+(\d+),\s+(\d+)\s+\]/g, "[$1, $2]")}\n`,
);
console.log(
  `Claude Code ${version} (${stripper.name}) from ${binPath}, sha256 ${sha256}: drops ${removed.length} code points ` +
    `in ${fixture.removed.length} ranges, reads ${newline.length} as a line break → ${outPath}`,
);
