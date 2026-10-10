// F-WB-03: fails when CHANGELOG.md breaks its format or has no section for
// the version being released (AC1). With --notes <mac|win|lin> it prints that
// version's notes for one platform instead (the update feed and the release
// page).
//   tsx scripts/check-changelog.ts [--version x.y.z] [--file CHANGELOG.md] [--notes lin]
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { notesFor, parseChangelog, validateChangelog, type Platform } from "../packages/protocol/src/changelog";

const root = new URL("..", import.meta.url).pathname;
const args = process.argv.slice(2);
const opt = (name: string) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};

const file = opt("--file") ?? join(root, "CHANGELOG.md");
const version =
  opt("--version") ??
  (JSON.parse(readFileSync(join(root, "apps/desktop/src-tauri/tauri.conf.json"), "utf8")) as { version: string })
    .version;
const text = readFileSync(file, "utf8");
const problems = validateChangelog(text);
const releases = parseChangelog(text);
if (!releases.some((r) => r.version === version)) problems.push(`CHANGELOG.md has no '## ${version}' section`);
if (problems.length) {
  for (const p of problems) console.error(`check-changelog: ${p}`);
  process.exit(1);
}
const platform = opt("--notes");
if (platform) {
  if (!["mac", "win", "lin"].includes(platform)) {
    console.error("check-changelog: --notes takes mac, win or lin");
    process.exit(2);
  }
  console.log(notesFor(releases, version, platform as Exclude<Platform, "all">));
} else {
  console.log(`CHANGELOG.md is valid and has ${version}`);
}
