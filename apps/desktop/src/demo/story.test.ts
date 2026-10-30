import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import NAMES from "../data/names.json";
import GIT_HUNKS from "./fixtures/git-hunks.json";
import { MAIN, changedIn, diffsOf, lineCount, testRun, testsIn } from "./repo";
import { STORY, askLine, askScreen, editsOf, previewOf, totalsOf, worktree, worktreeIn } from "./story";
import { unifiedDiff } from "./udiff";

describe("F-WB-01 D-2609 the sample world the demo and the site's scenes share (D-1899)", () => {
  it("F-WB-01 D-2609 opens with one agent done, one working and one waiting, each in a worktree of its own with its own engine", () => {
    expect(STORY.worktrees.map((w) => w.state).sort()).toEqual(["done", "waiting", "working"]);
    const slugs = STORY.worktrees.map((w) => w.slug);
    expect(new Set(slugs).size).toBe(slugs.length);
    expect(new Set(STORY.worktrees.map((w) => w.engine)).size).toBe(slugs.length);
    for (const w of STORY.worktrees) {
      expect(STORY.engines[w.engine]?.label, w.slug).toBeTruthy();
      expect(w.slug).toMatch(/^[a-z0-9]+(-[a-z0-9]+)*$/);
      expect(w.commits.length, w.slug).toBeGreaterThan(0);
    }
    // Every commit is its own: seven hex digits, never two the same.
    const shas = [
      ...STORY.history,
      ...STORY.worktrees.flatMap((w) => [...w.commits, { sha: w.mergeCommit, subject: "" }]),
      STORY.pr.fix.commit,
    ].map((c) => c.sha);
    for (const sha of shas) expect(sha).toMatch(/^[0-9a-f]{7}$/);
    expect(new Set(shas).size).toBe(shas.length);
    const done = worktreeIn("done");
    expect(done.closing).toBeTruthy();
    expect(done.steps.at(-1)).toMatchObject({ run: expect.any(String), passed: expect.any(Number) });
    expect(worktreeIn("working").steps.at(-1)).toMatchObject({ run: expect.any(String) });
    expect(worktreeIn("working").steps.at(-1)).not.toHaveProperty("passed");
    expect(worktreeIn("waiting").ask).toBeTruthy();
    expect(worktreeIn("waiting").closing).toBeTruthy();
  });

  it("F-WB-01 F-CD-12 each agent has a first name from the app's own list, none the same", () => {
    const names = STORY.worktrees.map((w) => w.agentName);
    for (const name of names) expect(NAMES, name).toContain(name);
    expect(new Set(names).size).toBe(names.length);
  });

  it("F-WB-01 D-2609 every worktree it names is its own, and the radar's symbol is in a file both its worktrees change", () => {
    expect(STORY.overlap.between).toHaveLength(2);
    expect(new Set(STORY.overlap.between).size).toBe(2);
    for (const slug of STORY.overlap.between) {
      expect(editsOf(worktree(slug)).map((e) => e.edit), slug).toContain(STORY.overlap.file);
      expect(changedIn(slug)[STORY.overlap.file], slug).toContain(STORY.overlap.symbol);
    }
    // Both change the symbol's definition: the radar's "changed here and there" (F-WT-10).
    const body = (text: string | undefined) => {
      const start = text?.indexOf(`export function ${STORY.overlap.symbol}(`) ?? -1;
      return start < 0 ? null : text?.slice(start, text.indexOf("\n}\n", start));
    };
    const base = body(MAIN[STORY.overlap.file]);
    expect(base).toBeTruthy();
    for (const slug of STORY.overlap.between) {
      const theirs = body(changedIn(slug)[STORY.overlap.file]);
      expect(theirs, slug).toBeTruthy();
      expect(theirs, slug).not.toBe(base);
    }
    // Second opinion is another engine reading a finished worktree's diff (F-WT-05).
    const reviewed = worktree(STORY.secondOpinion.worktree);
    expect(reviewed.state).toBe("done");
    expect(STORY.secondOpinion.engine).not.toBe(reviewed.engine);
    expect(STORY.engines[STORY.secondOpinion.engine]).toBeDefined();
    expect(worktree(STORY.pr.worktree).state).toBe("done");
    // The plan's tasks go to the engines the demo has, one each.
    expect(STORY.plan.tasks.map((t) => t.engine).sort()).toEqual(Object.keys(STORY.engines).sort());
    // Each comment sits on a line its worktree changed or kept, in a file it
    // changed: line 1 to its last line, never one past a final newline.
    for (const [slug, comments] of [
      [STORY.secondOpinion.worktree, STORY.secondOpinion.comments],
      [STORY.pr.worktree, STORY.pr.comments],
    ] as const) {
      for (const c of comments) {
        const text = changedIn(slug)[c.file];
        expect(text, `${slug} ${c.file}`).toBeDefined();
        expect(c.line, `${slug} ${c.file}`).toBeGreaterThanOrEqual(1);
        expect(c.line, `${slug} ${c.file}`).toBeLessThanOrEqual(lineCount(text ?? ""));
      }
    }
  });

  it("F-WB-01 D-2609 each edit's lines added and removed are what its file's diff counts", () => {
    for (const w of STORY.worktrees) {
      // A diff names a file once, so a worktree edits each file once.
      const files = editsOf(w).map((e) => e.edit);
      expect(new Set(files).size, w.slug).toBe(files.length);
      expect(Object.keys(changedIn(w.slug)).sort(), w.slug).toEqual([...files].sort());
      for (const d of diffsOf(w.slug)) {
        const step = editsOf(w).find((e) => e.edit === d.path);
        expect([d.additions, d.deletions], `${w.slug} ${d.path}`).toEqual([step?.add, step?.del]);
        expect(d.change, `${w.slug} ${d.path}`).toBe(MAIN[d.path] === undefined ? "added" : "modified");
      }
    }
    // What the review scene counts up: checkout-fix's 4 files, +62 −18.
    expect(totalsOf(worktree(STORY.secondOpinion.worktree))).toEqual({ files: 4, adds: 62, dels: 18 });
  });

  it("F-WB-01 D-2609 the tests the finished agent saw pass are the ones its checkout has", () => {
    const done = worktreeIn("done");
    const last = done.steps.at(-1);
    const run = last && "run" in last ? last : null;
    const filter = run?.run.replace(/^pnpm test ?/, "") ?? "";
    const files = { ...MAIN, ...changedIn(done.slug) };
    expect(testRun(files, filter).at(-1)).toBe(`      Tests  ${run?.passed} passed (${run?.passed})`);
  });

  it("F-WB-01 D-2609 the waiting agent asks in its own words, one string for the demo and the site's scenes", () => {
    const w = worktreeIn("waiting");
    // The engine's own question in its terminal: 0.12.0 raises no approval
    // for an engine's question, only for `vibezeo run` (D-1899). Gemini CLI
    // 0.66.0's question line, which registry.json's waitingPatterns match,
    // under the command in a box: no more of its screen than that (D-2633).
    expect(askLine(w)).toBe("Allow execution of [Shell]?");
    const rule = "─".repeat((w.ask ?? "").length + 2);
    expect(askScreen(w)).toEqual([`╭${rule}╮`, `│ ${w.ask} │`, `╰${rule}╯`, "Allow execution of [Shell]?"]);
    // Its card waits with no line: Gemini CLI has no hooks (D-2624).
    expect(previewOf(w)).toBeNull();
    // Nor do the others': Ada has finished, and Arlo's Codex names no
    // tools (drift.test.ts holds the rule to the app's sources).
    expect(previewOf(worktreeIn("done"))).toBeNull();
    expect(previewOf(worktreeIn("working"))).toBeNull();
  });

  it("F-WB-01 D-2633 the demo and the site's scenes quote another product only as far as the app needs", () => {
    // Gemini CLI's choices under its question, and what Claude Code's Edit
    // result says after its first sentence: the app reads nothing from them.
    const theirs = [/for this session/i, /suggest changes/i, /`cat -n`/, /snippet of the edited file/i];
    // Every source and sample file under the demo's folder and the scenes'
    // (the sample shop's too), tests aside, and nothing an editor or a tool
    // leaves there: no dot-files, dot-folders or node_modules.
    const test = /\.test\.tsx?$/;
    const source = /\.(tsx?|json|css|md)(\.sample)?$/;
    const under = (dir: string): string[] =>
      readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
        if (e.name.startsWith(".") || e.name === "node_modules") return [];
        if (e.isDirectory()) return under(join(dir, e.name));
        return source.test(e.name) && !test.test(e.name) ? [join(dir, e.name)] : [];
      });
    const dirs = [__dirname, join(__dirname, "../../../web/components/site/scenes")];
    const files = dirs.flatMap(under);
    // That's every file git tracks there, tests aside.
    const tracked = dirs.flatMap((dir) =>
      execFileSync("git", ["ls-files"], { cwd: dir, encoding: "utf8" })
        .split("\n")
        .filter((f) => f && !test.test(f))
        .map((f) => join(dir, f)),
    );
    expect(files).toEqual(expect.arrayContaining(tracked));
    expect(files.some((f) => f.endsWith("/scenes/DashboardScene.tsx"))).toBe(true);
    expect(files.some((f) => f.endsWith("/demo/story.ts"))).toBe(true);
    expect(files.some((f) => f.includes("/demo/shop/") && f.endsWith(".sample"))).toBe(true);
    expect(files.some((f) => f.includes("/demo/fixtures/"))).toBe(true);
    for (const file of files) {
      const text = readFileSync(file, "utf8");
      for (const words of theirs) expect(text, `${file} has ${words}`).not.toMatch(words);
    }
  });

  it("F-WB-01 D-2609 the pull request's checks are lint, test and build, with test failing", () => {
    expect(STORY.pr.checks.map((c) => [c.name, c.bucket])).toEqual([
      ["lint", "pass"],
      ["test", "fail"],
      ["build", "pass"],
    ]);
  });

  it("F-WB-01 D-2609 what would come from outside the computer says it's a sample, in the data", () => {
    expect(STORY.pr.title).toMatch(/^Sample PR: /);
    // On a reserved host (RFC 2606): nothing in the demo leads to a real repository.
    expect(STORY.remote).toMatch(/^git@github\.example:sample\//);
    expect(STORY.pr.url).toBe(`https://github.example/sample/${STORY.repo}/pull/${STORY.pr.number}`);
    for (const c of STORY.pr.comments) expect(c.author).toMatch(/sample/);
    for (const [id, e] of Object.entries(STORY.engines)) expect(e.version, id).toMatch(/^\d+\.\d+\.\d+-sample$/);
  });
});

describe("F-WB-01 F-WT-08 D-2614 the pull request's CI story", () => {
  const SESSION_TEST = "src/auth/session.test.ts";
  const SESSION = "src/auth/session.ts";
  const { pr } = STORY;
  const failed = pr.checks.find((c) => c.bucket === "fail");
  const log = failed?.log ?? [];
  const mainTests = MAIN[SESSION_TEST] ?? "";
  const lines = mainTests.split("\n");
  // CI runs the branch as pushed: main with the worktree's commits.
  const branch = { ...MAIN, ...changedIn(pr.worktree) };
  const testFiles = (files: Readonly<Record<string, string>>) =>
    Object.keys(files).filter((p) => /\.test\.tsx?$/.test(p));

  it("F-WB-01 F-WT-08 D-2614 CI's test job fails where the local run passed: main's tests call what the branch renamed", () => {
    expect(mainTests).toContain('import { getSession, signIn } from "./session";');
    expect(branch[SESSION]).not.toMatch(/export (async )?function getSession\b/);
    expect(branch[SESSION]).toMatch(/export async function refreshSession\b/);
    // The agent ran only the cart's tests, which don't touch the session's.
    const ran = worktree(pr.worktree).steps.at(-1);
    expect(ran).toMatchObject({ run: "pnpm test cart" });
    expect(testFiles(branch).filter((p) => p.includes("cart"))).not.toContain(SESSION_TEST);
    expect(changedIn(pr.worktree)[SESSION_TEST]).toBeUndefined();
  });

  it("F-WB-01 F-WT-08 D-2614 the failed job's log is vitest's on the samples: each of main's session tests, the call that throws, the totals", () => {
    const names = [...mainTests.matchAll(/^\s*it\("([^"]+)"/gm)].map((m) => m[1]);
    expect(names).toHaveLength(testsIn(mainTests));
    const prefix = ` FAIL  ${SESSION_TEST} > getSession > `;
    expect(log.filter((l) => l.startsWith(" FAIL  ")).map((l) => l.slice(prefix.length))).toEqual(names);
    expect(log.filter((l) => l === "TypeError: getSession is not a function")).toHaveLength(names.length);
    // Each failure points at its test's call to getSession: line and column.
    const calls = names.map((n) => {
      const start = lines.findIndex((l) => l.includes(`it("${n}"`));
      const at = lines.findIndex((l, i) => i > start && l.includes("getSession("));
      return ` ❯ ${SESSION_TEST}:${at + 1}:${(lines[at] ?? "").indexOf("getSession(") + 1}`;
    });
    expect(log.filter((l) => l.startsWith(` ❯ ${SESSION_TEST}:`))).toEqual(calls);
    // Its code frames quote main's file, cut short with … at the log's width.
    const frames = log.map((l) => /^ +(\d+)\| ?(.*)$/.exec(l)).filter((m) => m !== null);
    expect(frames.length).toBeGreaterThan(0);
    for (const [, n, text] of frames) {
      const line = lines[Number(n) - 1] ?? "";
      if (text?.endsWith("…")) expect(line.startsWith(text.slice(0, -1)), `${n}: ${text}`).toBe(true);
      else expect(line, `line ${n}`).toBe(text);
    }
    // The totals: main's session tests fail and every other file passes.
    const files = testFiles(branch);
    const failing = testsIn(mainTests);
    const passing = files.filter((p) => p !== SESSION_TEST).reduce((n, p) => n + testsIn(branch[p] ?? ""), 0);
    expect(log).toContain(` Test Files  1 failed | ${files.length - 1} passed (${files.length})`);
    expect(log).toContain(`      Tests  ${failing} failed | ${passing} passed (${failing + passing})`);
    expect(log.at(-1)).toBe("##[error]Process completed with exit code 1.");
  });

  it("F-WB-01 F-WT-08 D-2614 the fix is an edit of the test file, and then every test passes", () => {
    const fixed = changedIn("ci-fix")[SESSION_TEST] ?? "";
    expect(fixed).not.toContain("getSession");
    expect(fixed).toContain('import { refreshSession, signIn } from "./session";');
    const d = unifiedDiff(SESSION_TEST, mainTests, fixed);
    expect(pr.fix.steps).toEqual([
      { read: SESSION_TEST },
      { edit: SESSION_TEST, add: d.additions, del: d.deletions },
      { run: STORY.check, passed: testsIn(fixed) + testFiles(branch).filter((p) => p !== SESSION_TEST).reduce((n, p) => n + testsIn(branch[p] ?? ""), 0) },
    ]);
    const after = { ...branch, ...changedIn("ci-fix") };
    const run = pr.fix.steps.at(-1);
    const passed = run && "passed" in run ? run.passed : -1;
    expect(testRun(after).at(-1)).toBe(`      Tests  ${passed} passed (${passed})`);
    expect(pr.fix.closing).toContain(`${passed} tests pass`);
  });

  it("F-WB-01 F-WT-08 D-2614 the checks say where they ran: one CI run on the sample host, a job each; the fix's push starts the next", () => {
    for (const c of pr.checks) {
      expect(c.workflow).toBe("CI");
      expect(c.link).toMatch(new RegExp(`^https://github\\.example/sample/${STORY.repo}/actions/runs/\\d+/job/\\d+$`));
    }
    const runs = new Set(pr.checks.map((c) => /\/runs\/(\d+)\//.exec(c.link)?.[1]));
    expect(runs.size).toBe(1);
    expect(pr.fix.run).toBeGreaterThan(Number([...runs][0]));
    // Outside-world versions are a sample's, the log's vitest too.
    expect(log.find((l) => l.startsWith(" RUN  "))).toMatch(/^ RUN {2}v\d+\.\d+\.\d+-sample /);
  });
});

describe("F-WB-01 D-2609 the demo's test runs", () => {
  it("F-WB-01 D-2609 a test file counts its it() calls and each row of an it.each table", () => {
    const text = [
      'it.each([1, 2, 3])("one line", () => {});',
      "it.each([",
      '  [1, "a"],',
      '  [2, "b"],',
      '])("a row a line", () => {});',
      'it("one", () => {});',
      '  it("nested", () => {});',
    ].join("\n");
    expect(testsIn(text)).toBe(3 + 2 + 2);
  });

  it("F-WB-01 D-2609 pnpm test runs the test files its filter names, and says when none match", () => {
    const files = { "src/a.test.ts": 'it("x", () => {});\n', "src/b/c.test.tsx": 'it("y", () => {});\nit("z", () => {});\n', "src/a.ts": "" };
    expect(testRun(files, "b/")).toEqual([
      "> shop@1.4.0 test",
      "> vitest run b/",
      "",
      " ✓ src/b/c.test.tsx (2 tests) 12ms",
      "",
      " Test Files  1 passed (1)",
      "      Tests  2 passed (2)",
    ]);
    expect(testRun(files).at(-1)).toBe("      Tests  3 passed (3)");
    expect(testRun(files, "nope").slice(3)).toEqual(["No test files found, exiting with code 1", "filter: nope"]);
  });
});

describe("F-WB-01 D-2609 the demo's diffs", () => {
  it("F-WB-01 D-2609 a diff has git's header and hunks, three lines of context, and its counts", () => {
    const before = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"].join("\n") + "\n";
    const after = ["a", "b", "c", "D", "e", "f", "g", "h", "i", "j", "k"].join("\n") + "\n";
    expect(unifiedDiff("x.ts", before, after)).toEqual({
      additions: 2,
      deletions: 1,
      patch: [
        "diff --git a/x.ts b/x.ts",
        "--- a/x.ts",
        "+++ b/x.ts",
        "@@ -1,10 +1,11 @@",
        " a",
        " b",
        " c",
        "-d",
        "+D",
        " e",
        " f",
        " g",
        " h",
        " i",
        " j",
        "+k",
        "",
      ].join("\n"),
    });
  });

  it("F-WB-01 D-2609 changes six unchanged lines apart share a hunk, as git's do; seven apart get a hunk each", () => {
    const lines = Array.from({ length: 20 }, (_, i) => `${i + 1}`);
    const hunks = (changed: number[]) =>
      unifiedDiff("n.txt", `${lines.join("\n")}\n`, `${lines.map((l, i) => (changed.includes(i + 1) ? `${l}!` : l)).join("\n")}\n`)
        .patch.split("\n")
        .filter((l) => l.startsWith("@@"));
    expect(hunks([2, 9])).toEqual(["@@ -1,12 +1,12 @@"]);
    expect(hunks([2, 10])).toEqual(["@@ -1,5 +1,5 @@", "@@ -7,7 +7,7 @@"]);
  });

  it("F-WB-01 D-2609 changes far apart get a hunk each, and a new file is all additions", () => {
    const lines = Array.from({ length: 20 }, (_, i) => `line ${i + 1}`);
    const changed = lines.map((l, i) => (i === 1 || i === 17 ? `${l}!` : l));
    const { patch } = unifiedDiff("y.ts", `${lines.join("\n")}\n`, `${changed.join("\n")}\n`);
    // git names the last line before a hunk that starts with a letter, `_` or `$`.
    expect(patch.split("\n").filter((l) => l.startsWith("@@"))).toEqual(["@@ -1,5 +1,5 @@", "@@ -15,6 +15,6 @@ line 14"]);
    const added = unifiedDiff("z.ts", null, "one\ntwo\n");
    expect(added.patch).toBe(
      ["diff --git a/z.ts b/z.ts", "new file mode 100644", "--- /dev/null", "+++ b/z.ts", "@@ -0,0 +1,2 @@", "+one", "+two", ""].join("\n"),
    );
    expect([added.additions, added.deletions]).toEqual([2, 0]);
  });

  it("F-WB-01 D-2609 a hunk is named for the line above it git would name, or the hunk before's when none is between", () => {
    const lines = ["function f() {", ...Array.from({ length: 30 }, (_, i) => `  a${i + 1}`), "}"];
    const changed = lines.map((l) => (l === "  a5" || l === "  a25" ? `${l} // changed` : l));
    const { patch } = unifiedDiff("f.ts", `${lines.join("\n")}\n`, `${changed.join("\n")}\n`);
    // As `git diff --no-index` prints them.
    expect(patch.split("\n").filter((l) => l.startsWith("@@"))).toEqual([
      "@@ -3,7 +3,7 @@ function f() {",
      "@@ -23,7 +23,7 @@ function f() {",
    ]);
  });

  it("F-WB-01 D-2609 git's hunks come out the same whatever the developer's git setup: a repository's diff config and GIT_DIFF_OPTS change nothing", () => {
    const script = join(__dirname, "../../../../scripts/agent/demo-git-hunks.mjs");
    const tmp = mkdtempSync(join(tmpdir(), "demo-hunks-test-"));
    try {
      // Run from inside a repository whose config, and with a variable
      // whose value, would each change git's hunks if git read them.
      const git = (...args: string[]) => execFileSync("git", args, { cwd: tmp, stdio: "ignore" });
      git("init", "-q");
      git("config", "diff.interHunkContext", "10");
      git("config", "diff.suppressBlankEmpty", "true");
      const out = join(tmp, "hunks.json");
      execFileSync(process.execPath, [script, out], {
        cwd: tmp,
        env: { ...process.env, GIT_DIFF_OPTS: "--unified=1", GIT_DIR: join(tmp, ".git") },
        stdio: "ignore",
      });
      expect(JSON.parse(readFileSync(out, "utf8"))).toEqual(GIT_HUNKS);
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  }, 30_000);

  // A fake git is a shell script: POSIX's. CI runs these tests on Linux.
  it.skipIf(process.platform === "win32")(
    "F-WB-01 D-2609 git reads no system attributes, and a diff it prints without a hunk stops the script rather than writing a wrong fixture",
    () => {
      const script = join(__dirname, "../../../../scripts/agent/demo-git-hunks.mjs");
      const tmp = mkdtempSync(join(tmpdir(), "demo-hunks-test-"));
      try {
        // As git does for a file a system gitattributes calls `-diff`.
        const bin = join(tmp, "bin");
        mkdirSync(bin);
        writeFileSync(
          join(bin, "git"),
          `#!/bin/sh\nenv > "${join(tmp, "git-env")}"\necho "Binary files a and b differ"\nexit 1\n`,
          { mode: 0o755 },
        );
        const out = join(tmp, "hunks.json");
        expect(() =>
          execFileSync(process.execPath, [script, out], {
            env: { ...process.env, PATH: `${bin}:${process.env.PATH ?? ""}` },
            stdio: "pipe",
          }),
        ).toThrow(/no hunk/);
        expect(existsSync(out)).toBe(false);
        const env = readFileSync(join(tmp, "git-env"), "utf8").split("\n");
        expect(env).toContain("GIT_ATTR_NOSYSTEM=1");
        expect(env).toContain("GIT_CONFIG_NOSYSTEM=1");
      } finally {
        rmSync(tmp, { recursive: true, force: true });
      }
    },
  );

  it("F-WB-01 D-2609 every sample edit's diff is git's own, hunk for hunk (fixtures/git-hunks.json)", () => {
    const want = GIT_HUNKS as Record<string, string[]>;
    // Each place's every file is there: a new sample needs
    // `node scripts/agent/demo-git-hunks.mjs` run again.
    const places = [...STORY.worktrees.map((w) => w.slug), "thread", "ci-fix", "before"];
    expect(Object.keys(want).sort()).toEqual(
      places.flatMap((place) => Object.keys(changedIn(place)).map((path) => `${place}:${path}`)).sort(),
    );
    for (const [key, hunks] of Object.entries(want)) {
      const [place = "", path = ""] = key.split(":");
      // `before` holds main's files as they were: its diff runs to main.
      const d =
        place === "before"
          ? unifiedDiff(path, changedIn("before")[path] ?? "", MAIN[path] ?? "")
          : unifiedDiff(path, MAIN[path] ?? null, changedIn(place)[path] ?? "");
      const lines = d.patch.split("\n");
      expect(lines.slice(lines.findIndex((l) => l.startsWith("@@")), -1), key).toEqual(hunks);
    }
  });
});
