// Writes git's own hunks for every edit the browser demo diffs (its sample
// repository, apps/desktop/src/demo/shop/), so story.test.ts can hold the
// demo's diffs to git's: each worktree's, the first thread's and the CI
// fix's files against main, and main's against `before`. Run it after a
// sample changes:
//   node scripts/agent/demo-git-hunks.mjs [out.json]
// git runs in a folder of its own with PATH and an empty HOME and nothing
// else: no repository around it, no global or system config or attributes,
// and no GIT_* variable, so nothing in the developer's setup (a repository's
// diff.interHunkContext, GIT_DIFF_OPTS) changes the hunks.
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";

const demo = new URL("../../apps/desktop/src/demo/", import.meta.url).pathname;
const shop = join(demo, "shop");
const out = process.argv[2] ? resolve(process.argv[2]) : join(demo, "fixtures", "git-hunks.json");

/** A place's sample files, by path. */
function files(place) {
  const root = join(shop, place);
  const found = {};
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const at = join(dir, name);
      if (statSync(at).isDirectory()) walk(at);
      else if (name.endsWith(".sample"))
        found[relative(root, at).slice(0, -".sample".length)] = readFileSync(at, "utf8");
    }
  };
  walk(root);
  return found;
}

/** git's hunks for `before` → `after`: its lines from the first `@@`. */
function hunks(before, after) {
  const dir = mkdtempSync(join(tmpdir(), "demo-hunks-"));
  try {
    writeFileSync(join(dir, "a"), before ?? "");
    writeFileSync(join(dir, "b"), after);
    const env = {
      PATH: process.env.PATH ?? "",
      HOME: dir,
      LC_ALL: "C",
      GIT_CONFIG_NOSYSTEM: "1",
      GIT_CONFIG_GLOBAL: "/dev/null",
      // A system gitattributes' `* -diff` makes every file binary to git.
      GIT_ATTR_NOSYSTEM: "1",
      // No repository above the folder is found, wherever tmp is.
      GIT_CEILING_DIRECTORIES: dirname(dir),
    };
    let text = "";
    try {
      execFileSync(
        "git",
        [
          "diff",
          "--no-index",
          "--no-color",
          "--no-ext-diff",
          "--diff-algorithm=myers",
          "--indent-heuristic",
          "-U3",
          before === null ? "/dev/null" : "a",
          "b",
        ],
        { cwd: dir, encoding: "utf8", env },
      );
    } catch (e) {
      // `git diff` exits 1 when the files differ.
      if (e.status !== 1) throw e;
      text = e.stdout;
    }
    const lines = text.split("\n");
    if (lines.at(-1) === "") lines.pop();
    // The same file: git printed nothing.
    if (lines.length === 0) return [];
    const first = lines.findIndex((l) => l.startsWith("@@"));
    // A diff without a hunk ("Binary files a and b differ") is no fixture.
    if (first < 0) throw new Error(`git printed no hunk: ${lines.at(-1)}`);
    return lines.slice(first);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const main = files("main");
const result = {};
for (const place of readdirSync(shop).sort()) {
  if (place === "main") continue;
  for (const [path, text] of Object.entries(files(place)).sort(([x], [y]) => x.localeCompare(y))) {
    // `before` holds main's files as they were: its diff runs the other way.
    result[`${place}:${path}`] =
      place === "before" ? hunks(text, main[path] ?? "") : hunks(main[path] ?? null, text);
  }
}
writeFileSync(out, `${JSON.stringify(result, null, 2)}\n`);
console.log(`${Object.keys(result).length} diffs → ${relative(process.cwd(), out)}`);
