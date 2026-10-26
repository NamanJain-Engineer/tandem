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

/** An Added entry as its card shows it (D-1887): the bold title, the first
 * sentence after it (the lead, in fg) and the rest (the detail, muted). */
export function addedCard(text: string): { title: string; lead: string; rest: string } {
  const m = /^\*\*(.+?)\*\*\s*([\s\S]*)$/.exec(text);
  const title = m ? (m[1] ?? "") : "";
  const body = m ? (m[2] ?? "") : text;
  const end = body.search(/[.!?]["”)]?\s/);
  if (end < 0) return { title, lead: body.trim(), rest: "" };
  const cut = body.slice(end).search(/\s/) + end;
  return { title, lead: body.slice(0, cut).trim(), rest: body.slice(cut).trim() };
}
