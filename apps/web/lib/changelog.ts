// The website's changelog (F-WB-03), read from the repository's
// CHANGELOG.md at build time: the index and one static page per version.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { parseChangelog, type Platform, type Release } from "@tandem/protocol/changelog";

/** CHANGELOG.md at the repository root, found upward from where the build
 * runs (apps/web for Next and vitest alike). */
function changelogPath(): string {
  let dir = process.cwd();
  for (let i = 0; i < 6; i++) {
    const p = join(dir, "CHANGELOG.md");
    if (existsSync(p)) return p;
    dir = dirname(dir);
  }
  throw new Error("CHANGELOG.md not found above " + process.cwd());
}

export const RELEASES: readonly Release[] = parseChangelog(readFileSync(changelogPath(), "utf8"));

export const versionPath = (version: string) => `/changelog/${version}/`;

export const PLATFORM_LABEL: Record<Platform, string> = {
  all: "All platforms",
  lin: "Linux",
  mac: "macOS",
  win: "Windows",
};
