import { describe, expect, it } from "vitest";
import { MAIN, changedIn, diffsOf, testRun, testsIn } from "./repo";
import { STORY, editsOf, totalsOf, worktree, worktreeIn } from "./story";
import { unifiedDiff } from "./udiff";

describe("D-2609 the sample world the demo and the site's scenes share (D-1899)", () => {
  it("D-2609 opens with one agent done, one working and one waiting, each in a worktree of its own with its own engine", () => {
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
    ].map((c) => c.sha);
    for (const sha of shas) expect(sha).toMatch(/^[0-9a-f]{7}$/);
    expect(new Set(shas).size).toBe(shas.length);
    const done = worktreeIn("done");
    expect(done.closing).toBeTruthy();
    expect(done.steps.at(-1)).toMatchObject({ run: expect.any(String), passed: expect.any(Number) });
    expect(worktreeIn("working").steps.at(-1)).toMatchObject({ run: expect.any(String) });
    expect(worktreeIn("working").steps.at(-1)).not.toHaveProperty("passed");
    expect(worktreeIn("waiting").ask).toBeTruthy();
  });

  it("D-2609 every worktree it names is its own, and the radar's symbol is in a file both its worktrees change", () => {
    expect(STORY.overlap.between).toHaveLength(2);
    expect(new Set(STORY.overlap.between).size).toBe(2);
    for (const slug of STORY.overlap.between) {
      expect(editsOf(worktree(slug)).map((e) => e.edit), slug).toContain(STORY.overlap.file);
      expect(changedIn(slug)[STORY.overlap.file], slug).toContain(STORY.overlap.symbol);
    }
    // Second opinion is another engine reading a finished worktree's diff (F-WT-05).
    const reviewed = worktree(STORY.secondOpinion.worktree);
    expect(reviewed.state).toBe("done");
    expect(STORY.secondOpinion.engine).not.toBe(reviewed.engine);
    expect(STORY.engines[STORY.secondOpinion.engine]).toBeDefined();
    expect(worktree(STORY.pr.worktree).state).toBe("done");
    // The plan's tasks go to the engines the demo has, one each.
    expect(STORY.plan.tasks.map((t) => t.engine).sort()).toEqual(Object.keys(STORY.engines).sort());
    // Each comment sits on a line its worktree changed or kept, in a file it changed.
    for (const [slug, comments] of [
      [STORY.secondOpinion.worktree, STORY.secondOpinion.comments],
      [STORY.pr.worktree, STORY.pr.comments],
    ] as const) {
      for (const c of comments) {
        const text = changedIn(slug)[c.file];
        expect(text, `${slug} ${c.file}`).toBeDefined();
        expect(c.line).toBeLessThanOrEqual(text?.split("\n").length ?? 0);
      }
    }
  });

  it("D-2609 each edit's lines added and removed are what its file's diff counts", () => {
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

  it("D-2609 the tests the finished agent saw pass are the ones its checkout has", () => {
    const done = worktreeIn("done");
    const last = done.steps.at(-1);
    const run = last && "run" in last ? last : null;
    const filter = run?.run.replace(/^pnpm test ?/, "") ?? "";
    const files = { ...MAIN, ...changedIn(done.slug) };
    expect(testRun(files, filter).at(-1)).toBe(`      Tests  ${run?.passed} passed (${run?.passed})`);
  });

  it("D-2609 the pull request's checks are lint, test and build, with test failing", () => {
    expect(STORY.pr.checks.map((c) => [c.name, c.bucket])).toEqual([
      ["lint", "pass"],
      ["test", "fail"],
      ["build", "pass"],
    ]);
  });

  it("D-2609 what would come from outside the computer says it's a sample, in the data", () => {
    expect(STORY.pr.title).toMatch(/^Sample PR: /);
    // On a reserved host (RFC 2606): nothing in the demo leads to a real repository.
    expect(STORY.remote).toMatch(/^git@github\.example:sample\//);
    expect(STORY.pr.url).toBe(`https://github.example/sample/${STORY.repo}/pull/${STORY.pr.number}`);
    for (const c of STORY.pr.comments) expect(c.author).toMatch(/sample/);
    for (const [id, e] of Object.entries(STORY.engines)) expect(e.version, id).toMatch(/^\d+\.\d+\.\d+-sample$/);
  });
});

describe("D-2609 the demo's test runs", () => {
  it("D-2609 a test file counts its it() calls and each row of an it.each table", () => {
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

  it("D-2609 pnpm test runs the test files its filter names, and says when none match", () => {
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

describe("D-2609 the demo's diffs", () => {
  it("D-2609 a diff has git's header and hunks, three lines of context, and its counts", () => {
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

  it("D-2609 changes six unchanged lines apart share a hunk, as git's do; seven apart get a hunk each", () => {
    const lines = Array.from({ length: 20 }, (_, i) => `${i + 1}`);
    const hunks = (changed: number[]) =>
      unifiedDiff("n.txt", `${lines.join("\n")}\n`, `${lines.map((l, i) => (changed.includes(i + 1) ? `${l}!` : l)).join("\n")}\n`)
        .patch.split("\n")
        .filter((l) => l.startsWith("@@"));
    expect(hunks([2, 9])).toEqual(["@@ -1,12 +1,12 @@"]);
    expect(hunks([2, 10])).toEqual(["@@ -1,5 +1,5 @@", "@@ -7,7 +7,7 @@"]);
  });

  it("D-2609 changes far apart get a hunk each, and a new file is all additions", () => {
    const lines = Array.from({ length: 20 }, (_, i) => `line ${i + 1}`);
    const changed = lines.map((l, i) => (i === 1 || i === 17 ? `${l}!` : l));
    const { patch } = unifiedDiff("y.ts", `${lines.join("\n")}\n`, `${changed.join("\n")}\n`);
    expect(patch.split("\n").filter((l) => l.startsWith("@@"))).toEqual(["@@ -1,5 +1,5 @@", "@@ -15,6 +15,6 @@"]);
    const added = unifiedDiff("z.ts", null, "one\ntwo\n");
    expect(added.patch).toBe(
      ["diff --git a/z.ts b/z.ts", "new file mode 100644", "--- /dev/null", "+++ b/z.ts", "@@ -0,0 +1,2 @@", "+one", "+two", ""].join("\n"),
    );
    expect([added.additions, added.deletions]).toEqual([2, 0]);
  });
});
