#!/usr/bin/env tsx
/**
 * THIRD_PARTY_NOTICES.md (F-TL-04 AC2, ARCHITECTURE §17.1).
 *
 *   pnpm tsx scripts/gen-notices.ts [--out dist] [--resources]
 *
 * Sources:
 * - Rust: `cargo about generate --format json` for the desktop app's crates
 *   (licences accepted in about.toml), with each licence's text;
 * - npm: `pnpm --filter "desktop..." licenses list --prod --json` (the app and its workspace packages), with the
 *   packages' own LICENSE files;
 * - `crates/tandem-voice/models/catalog.json` (downloadable models and
 *   their attribution: Parakeet is CC-BY-4.0);
 * - `licenses/extra-attributions.toml` (models, OFL fonts, and the LGPL
 *   libraries the Linux AppImage bundles).
 *
 * Writes `<out>/THIRD_PARTY_NOTICES.md`; `--resources` also copies it to
 * the desktop's resources, where About › Licenses reads it.
 */
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { parseToml, type TomlValue } from "./lib/toml";

const root = resolve(import.meta.dirname, "..");
const args = process.argv.slice(2);
const option = (name: string, fallback: string) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? (args[i + 1] as string) : fallback;
};
const out = resolve(root, option("out", "dist"));
const version = (
  JSON.parse(readFileSync(join(root, "apps/desktop/src-tauri/tauri.conf.json"), "utf8")) as {
    version: string;
  }
).version;

type Row = Record<string, TomlValue>;
const extra = parseToml(readFileSync(join(root, "licenses/extra-attributions.toml"), "utf8"));
const rows = (k: string) => (extra[k] as Row[] | undefined) ?? [];
type Model = { id: string; license: string; attribution: string; source?: string };
const catalog = JSON.parse(readFileSync(join(root, "crates/tandem-voice/models/catalog.json"), "utf8")) as {
  models: Model[];
};

const problems: string[] = [];
const lines: string[] = [];
const push = (...l: string[]) => lines.push(...l);

push(
  `# Third-party notices`,
  "",
  `Tandem ${version} includes or downloads the software and data listed here. Each is used under its own licence, reproduced or linked below.`,
  "",
);

// ── Models (downloaded on request) ────────────────────────────────────────
push("## Speech models (downloaded when you turn on Dictate)", "");
for (const m of catalog.models) {
  const entry = rows("model").find((r) => r.id === m.id);
  if (!entry) problems.push(`model ${m.id} has no entry in licenses/extra-attributions.toml`);
  else if (entry.license !== m.license)
    problems.push(`model ${m.id}: catalog says ${m.license}, attributions say ${String(entry.license)}`);
  push(
    `### ${entry ? String(entry.name) : m.id}`,
    "",
    `Licence: ${m.license}${m.source ? ` · ${m.source}` : ""}`,
    "",
    m.attribution,
    "",
  );
}

// ── Models bundled with the app (the "Hey Conductor" keyword spotter) ─────
const bundledModels = rows("model").filter((r) => r.bundled === true);
if (bundledModels.length) {
  push("## Models bundled with the app", "");
  for (const r of bundledModels)
    push(`### ${String(r.name)}`, "", `Licence: ${String(r.license)} · ${String(r.url)}`, "", String(r.copyright), "");
}
const cc = "/usr/share/common-licenses/CC-BY-4.0";
push(
  "Creative Commons Attribution 4.0 International: https://creativecommons.org/licenses/by/4.0/legalcode",
  "",
  ...(existsSync(cc) ? ["```", readFileSync(cc, "utf8").trim(), "```", ""] : []),
);

// ── Fonts ─────────────────────────────────────────────────────────────────
push("## Fonts", "");
for (const f of rows("font")) {
  push(
    `### ${String(f.name)}`,
    "",
    `${String(f.copyright)}`,
    "",
    `Licence: ${String(f.license)} (SIL Open Font License 1.1)`,
    "",
  );
  const pkgDir = join(root, "packages/ui/node_modules", String(f.package));
  const licence = existsSync(pkgDir)
    ? readdirSync(pkgDir).find((n) => /^(LICEN[CS]E|OFL)/i.test(n))
    : undefined;
  if (licence) push("```", readFileSync(join(pkgDir, licence), "utf8").trim(), "```", "");
  else problems.push(`font ${String(f.name)}: no licence file under ${pkgDir}`);
}

// ── LGPL libraries in the Linux AppImage ─────────────────────────────────
push(
  "## Libraries bundled in the Linux AppImage",
  "",
  "The AppImage carries these system libraries so Tandem runs on older distributions. They are licensed under the GNU Lesser General Public License (LGPL); you may obtain their source from the projects below and replace the bundled copies under `usr/lib` inside the AppImage.",
  "",
);
for (const b of rows("bundled")) push(`- **${String(b.name)}**: ${String(b.license)} · ${String(b.url)}`);
push("");
const lgpl = "/usr/share/common-licenses/LGPL-2.1";
push("### GNU Lesser General Public License, version 2.1", "");
if (existsSync(lgpl)) push("```", readFileSync(lgpl, "utf8").trim(), "```", "");
else push("https://www.gnu.org/licenses/old-licenses/lgpl-2.1.txt", "");

// ── Rust crates ──────────────────────────────────────────────────────────
type About = {
  licenses: Array<{
    name: string;
    id: string;
    text: string;
    used_by: Array<{ crate: { name: string; version: string } }>;
  }>;
};
push("## Rust crates", "");
const haveAbout = spawnSync("cargo", ["about", "--version"], { encoding: "utf8" }).status === 0;
if (!haveAbout) {
  problems.push("cargo-about isn't installed (`cargo install --locked cargo-about`)");
} else {
  const json = execFileSync(
    "cargo",
    [
      "about",
      "generate",
      "--format",
      "json",
      "-c",
      join(root, "about.toml"),
      "-m",
      join(root, "apps/desktop/src-tauri/Cargo.toml"),
    ],
    { cwd: root, encoding: "utf8", maxBuffer: 256 * 1024 * 1024, stdio: ["ignore", "pipe", "pipe"] },
  );
  const about = JSON.parse(json) as About;
  // Same licence and text: one block listing every crate that uses it.
  const groups = new Map<string, { id: string; name: string; text: string; crates: Set<string> }>();
  for (const l of about.licenses) {
    const key = `${l.id}\n${l.text.trim()}`;
    const g = groups.get(key) ?? { id: l.id, name: l.name, text: l.text.trim(), crates: new Set<string>() };
    for (const u of l.used_by) g.crates.add(`${u.crate.name} ${u.crate.version}`);
    groups.set(key, g);
  }
  for (const g of [...groups.values()].sort(
    (a, b) => a.id.localeCompare(b.id) || b.crates.size - a.crates.size,
  )) {
    push(
      `### ${g.name} (${g.id})`,
      "",
      `Used by: ${[...g.crates].sort().join(", ")}`,
      "",
      "```",
      g.text,
      "```",
      "",
    );
  }
}

// ── npm packages ─────────────────────────────────────────────────────────
type PnpmLicences = Record<string, Array<{ name: string; versions: string[]; paths: string[] }>>;
push("## JavaScript packages", "");
const pnpm = spawnSync("pnpm", ["--filter", "desktop...", "licenses", "list", "--prod", "--json"], {
  cwd: root,
  encoding: "utf8",
  maxBuffer: 64 * 1024 * 1024,
});
if (pnpm.status !== 0) {
  problems.push(`pnpm licenses failed: ${pnpm.stderr.split("\n")[0] ?? ""}`);
} else {
  const byLicence = JSON.parse(pnpm.stdout) as PnpmLicences;
  for (const [licence, pkgs] of Object.entries(byLicence).sort(([a], [b]) => a.localeCompare(b))) {
    push(`### ${licence}`, "");
    const texts = new Map<string, string[]>();
    for (const p of pkgs.sort((a, b) => a.name.localeCompare(b.name))) {
      const label = `${p.name} ${p.versions.join(", ")}`;
      const dir = p.paths[0];
      const file =
        dir && existsSync(dir)
          ? readdirSync(dir).find((n) => /^(LICEN[CS]E|COPYING)(\.|$)/i.test(n))
          : undefined;
      const text = file && dir ? readFileSync(join(dir, file), "utf8").trim() : "";
      const list = texts.get(text) ?? [];
      list.push(label);
      texts.set(text, list);
    }
    for (const [text, names] of texts) {
      push(`Used by: ${names.join(", ")}`, "");
      if (text) push("```", text, "```", "");
    }
  }
}

if (problems.length > 0) {
  for (const p of problems) console.error(`gen-notices: ${p}`);
  process.exit(1);
}
mkdirSync(out, { recursive: true });
const body = lines.join("\n").trimEnd() + "\n";
writeFileSync(join(out, "THIRD_PARTY_NOTICES.md"), body);
if (args.includes("--resources")) {
  const res = join(root, "apps/desktop/src-tauri/resources");
  mkdirSync(res, { recursive: true });
  writeFileSync(join(res, "THIRD_PARTY_NOTICES.md"), body);
}
console.log(`THIRD_PARTY_NOTICES.md: ${body.length.toLocaleString()} bytes → ${out}`);
