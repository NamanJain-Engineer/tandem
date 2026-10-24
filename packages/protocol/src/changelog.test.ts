import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { notesFor, parseChangelog, validateChangelog } from "./changelog";

const SAMPLE = `# Changelog

Intro text.

## 1.2.0

Linux and macOS.

### Added
- [all] **Threads.** Talk to agents
  as a conversation.
- [mac][lin] A Linux and macOS thing.

### Fixed
- [win] A Windows fix.

## 1.1.0

Not released.
`;

describe("CHANGELOG.md format (F-WB-03)", () => {
  it("F-WB-03 parses versions, summaries, categories, platform tags and wrapped bullets", () => {
    const releases = parseChangelog(SAMPLE);
    expect(releases.map((r) => r.version)).toEqual(["1.2.0", "1.1.0"]);
    const [r] = releases;
    expect(r?.summary).toBe("Linux and macOS.");
    expect(r?.entries).toEqual([
      { category: "Added", platforms: ["all"], text: "**Threads.** Talk to agents as a conversation." },
      { category: "Added", platforms: ["mac", "lin"], text: "A Linux and macOS thing." },
      { category: "Fixed", platforms: ["win"], text: "A Windows fix." },
    ]);
    expect(releases[1]?.entries).toEqual([]);
  });

  it("F-WB-03 notes for one platform keep its entries and [all], in category order", () => {
    const notes = notesFor(parseChangelog(SAMPLE), "1.2.0", "lin");
    expect(notes).toBe("Linux and macOS.\n\nAdded\n- **Threads.** Talk to agents as a conversation.\n- A Linux and macOS thing.");
    expect(notesFor(parseChangelog(SAMPLE), "9.9.9", "lin")).toBeNull();
  });

  it("F-WB-03 validation names every problem", () => {
    const bad = `## 1.0.0\n\n### Removed\n- [lin] x\n\n### Added\n- no tag\n- [bsd] odd tag\n\n## 1.0.0\n\n## v2\n`;
    expect(validateChangelog(bad)).toEqual([
      '1.0.0: unknown category "Removed" (Added, Changed, Fixed, Security)',
      '1.0.0: a bullet without a platform tag ([mac], [win], [lin] or [all]): "no tag"',
      '1.0.0: unknown platform tag [bsd] in "[bsd] odd tag"',
      "1.0.0: the version appears twice",
      'v2: "v2" isn\'t a version (x.y.z)',
    ]);
  });

  it("F-WB-03 a first ## Unreleased section is checked but is no release (D-2906)", () => {
    const text = `## Unreleased\n\nNext.\n\n### Changed\n- [all] Everything free.\n\n${SAMPLE.slice(SAMPLE.indexOf("## 1.2.0"))}`;
    expect(validateChangelog(text)).toEqual([]);
    expect(parseChangelog(text).map((r) => r.version)).not.toContain("Unreleased");
    expect(parseChangelog(text)[0]?.version).toBe("1.2.0");
    expect(validateChangelog(`## Unreleased\n\n### Added\n- no tag\n`)).toEqual([
      'Unreleased: a bullet without a platform tag ([mac], [win], [lin] or [all]): "no tag"',
    ]);
    expect(validateChangelog(`## 1.0.0\n\n## Unreleased\n`)).toEqual(["Unreleased: it must be the first section"]);
  });

  it("F-WB-03 the repository's CHANGELOG.md is valid", () => {
    const text = readFileSync(join(import.meta.dirname, "../../../CHANGELOG.md"), "utf8");
    expect(validateChangelog(text)).toEqual([]);
    expect(parseChangelog(text)[0]?.entries.length).toBeGreaterThan(0);
  });
});

describe("scripts/check-changelog.ts (F-WB-03 AC1)", () => {
  const run = (args: string[]) =>
    spawnSync(join(import.meta.dirname, "../../../node_modules/.bin/tsx"), [
      join(import.meta.dirname, "../../../scripts/check-changelog.ts"),
      ...args,
    ], { encoding: "utf8" });
  const write = (text: string) => {
    const dir = mkdtempSync(join(tmpdir(), "changelog-"));
    const file = join(dir, "CHANGELOG.md");
    writeFileSync(file, text);
    return file;
  };

  it("F-WB-03.1 a release fails when its version has no CHANGELOG section", () => {
    const r = run(["--file", write(SAMPLE), "--version", "1.3.0"]);
    expect(r.status).toBe(1);
    expect(r.stderr).toContain("CHANGELOG.md has no '## 1.3.0' section");
    expect(run(["--file", write(SAMPLE), "--version", "1.2.0"]).status).toBe(0);
  });

  it("F-WB-03 a malformed CHANGELOG fails the check too, and --notes prints one platform's notes", () => {
    expect(run(["--file", write("## 1.0.0\n\n### Added\n- untagged\n"), "--version", "1.0.0"]).status).toBe(1);
    const notes = run(["--file", write(SAMPLE), "--version", "1.2.0", "--notes", "win"]);
    expect(notes.stdout.trim()).toBe("Linux and macOS.\n\nAdded\n- **Threads.** Talk to agents as a conversation.\n\nFixed\n- A Windows fix.");
  });
});
