// CHANGELOG.md (F-WB-03): the one source for release notes. Each version is
// a `## x.y.z` section with an optional summary, then `### Added`,
// `### Changed`, `### Fixed` or `### Security`, whose bullets start with
// platform tags: `[mac]`, `[win]`, `[lin]` or `[all]`. It feeds the
// website's changelog pages, the app's "What's new" and the update feed.
// A `## Unreleased` section, first in the file, gathers notes for the next
// release: it's checked like any other but is no release, so nothing shows
// it until it's renamed to its version (D-2906).

export const CATEGORIES = ["Added", "Changed", "Fixed", "Security"] as const;
export type Category = (typeof CATEGORIES)[number];
export const PLATFORMS = ["mac", "win", "lin", "all"] as const;
export type Platform = (typeof PLATFORMS)[number];

export type ChangelogEntry = { category: Category; platforms: Platform[]; text: string };
export type Release = { version: string; summary: string; entries: ChangelogEntry[] };

const VERSION = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?$/;
/** The heading of notes not yet released. */
export const UNRELEASED = "Unreleased";
const TAGS = /^((?:\[[a-z]+\])+)\s*/;

type Raw = {
  version: string;
  summary: string[];
  entries: Array<ChangelogEntry & { raw: string }>;
  problems: string[];
};

function scan(text: string): Raw[] {
  const out: Raw[] = [];
  let cur: Raw | null = null;
  let category: string | null = null;
  let last: (ChangelogEntry & { raw: string }) | null = null;
  for (const line of text.split(/\r?\n/)) {
    if (line.startsWith("## ")) {
      const version = line.slice(3).trim().split(/\s+/)[0] ?? "";
      cur = { version, summary: [], entries: [], problems: [] };
      out.push(cur);
      category = null;
      last = null;
      continue;
    }
    if (!cur) continue;
    if (line.startsWith("### ")) {
      category = line.slice(4).trim();
      if (!(CATEGORIES as readonly string[]).includes(category))
        cur.problems.push(`unknown category "${category}" (${CATEGORIES.join(", ")})`);
      last = null;
      continue;
    }
    if (line.startsWith("- ") && category) {
      const body = line.slice(2).trim();
      const m = TAGS.exec(body);
      if (!m) {
        cur.problems.push(`a bullet without a platform tag ([mac], [win], [lin] or [all]): "${body}"`);
        last = null;
        continue;
      }
      const tags = [...(m[1] ?? "").matchAll(/\[([a-z]+)\]/g)].map((t) => t[1] ?? "");
      const unknown = tags.find((t) => !(PLATFORMS as readonly string[]).includes(t));
      if (unknown !== undefined) {
        cur.problems.push(`unknown platform tag [${unknown}] in "${body}"`);
        last = null;
        continue;
      }
      last = {
        category: category as Category,
        platforms: tags as Platform[],
        text: body.slice(m[0].length),
        raw: body,
      };
      if ((CATEGORIES as readonly string[]).includes(category)) cur.entries.push(last);
      continue;
    }
    if (/^\s+\S/.test(line) && last) {
      last.text += ` ${line.trim()}`;
      continue;
    }
    if (line.trim() === "") {
      if (!category && cur.summary.length && cur.summary.at(-1) !== "") cur.summary.push("");
      last = null;
      continue;
    }
    if (!category) cur.summary.push(line.trim());
  }
  return out;
}

function summaryOf(lines: string[]): string {
  return lines
    .join("\n")
    .split(/\n\n+/)
    .map((p) => p.split("\n").join(" ").trim())
    .filter(Boolean)
    .join("\n\n");
}

/** Every released version, newest first as written (`## Unreleased` is left out). */
export function parseChangelog(text: string): Release[] {
  return scan(text)
    .filter((r) => r.version !== UNRELEASED)
    .map((r) => ({
      version: r.version,
      summary: summaryOf(r.summary),
      entries: r.entries.map(({ raw: _raw, ...e }) => e),
    }));
}

/** What's wrong with the file, one line per problem; empty when it's valid. */
export function validateChangelog(text: string): string[] {
  const problems: string[] = [];
  const seen = new Set<string>();
  for (const [i, r] of scan(text).entries()) {
    for (const p of r.problems) problems.push(`${r.version}: ${p}`);
    if (seen.has(r.version)) problems.push(`${r.version}: the version appears twice`);
    seen.add(r.version);
    if (r.version === UNRELEASED) {
      if (i > 0) problems.push(`${UNRELEASED}: it must be the first section`);
    } else if (!VERSION.test(r.version))
      problems.push(`${r.version}: "${r.version}" isn't a version (x.y.z)`);
  }
  return problems;
}

/** One version's notes for one platform (plain text, for the update feed
 * and "What's new"); `null` when the version has no section. */
export function notesFor(
  releases: Release[],
  version: string,
  platform: Exclude<Platform, "all">,
): string | null {
  const r = releases.find((x) => x.version === version);
  if (!r) return null;
  const parts = r.summary ? [r.summary] : [];
  for (const c of CATEGORIES) {
    const lines = r.entries
      .filter((e) => e.category === c && (e.platforms.includes("all") || e.platforms.includes(platform)))
      .map((e) => `- ${e.text}`);
    if (lines.length) parts.push([c, ...lines].join("\n"));
  }
  return parts.join("\n\n");
}
