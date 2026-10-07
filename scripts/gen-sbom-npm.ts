#!/usr/bin/env tsx
/**
 * CycloneDX 1.5 SBOM of the JavaScript the desktop app ships (F-TL-04).
 *
 *   pnpm tsx scripts/gen-sbom-npm.ts --out dist/tandem-npm.cdx.json
 *
 * Built from `pnpm --filter "desktop..." licenses list --prod --json` (the app and its workspace packages): pnpm's own
 * production dependency set. `@cyclonedx/cyclonedx-npm` reads `npm ls`,
 * which misreads pnpm's node_modules (missing-dependency errors, dev
 * packages listed), so it isn't used.
 */
import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const args = process.argv.slice(2);
const i = args.indexOf("--out");
const out = resolve(root, i >= 0 && args[i + 1] ? (args[i + 1] as string) : "dist/tandem-npm.cdx.json");
const app = JSON.parse(readFileSync(join(root, "apps/desktop/package.json"), "utf8")) as {
  name: string;
  version: string;
};

const r = spawnSync("pnpm", ["--filter", "desktop...", "licenses", "list", "--prod", "--json"], {
  cwd: root,
  encoding: "utf8",
  maxBuffer: 64 * 1024 * 1024,
});
if (r.status !== 0) {
  console.error(`gen-sbom-npm: pnpm licenses failed: ${r.stderr}`);
  process.exit(1);
}
type Pkg = { name: string; versions: string[]; homepage?: string; description?: string };
const byLicence = JSON.parse(r.stdout) as Record<string, Pkg[]>;

/** `pkg:npm/%40scope/name@1.2.3` */
const purl = (name: string, version: string) =>
  `pkg:npm/${name.startsWith("@") ? `%40${name.slice(1)}` : name}@${version}`;
/** SPDX ids go in `license.id`; anything else (e.g. "SEE LICENSE IN …") in `name`. */
const licence = (l: string) =>
  /^[A-Za-z0-9.+-]+$/.test(l) && !/^unknown$/i.test(l) ? { license: { id: l } } : { license: { name: l } };

const components = Object.entries(byLicence)
  .flatMap(([lic, pkgs]) =>
    pkgs.flatMap((p) =>
      p.versions.map((v) => ({
        type: "library",
        "bom-ref": purl(p.name, v),
        name: p.name,
        version: v,
        purl: purl(p.name, v),
        licenses: lic.includes(" OR ") || lic.includes(" AND ") ? [{ expression: lic }] : [licence(lic)],
        ...(p.description ? { description: p.description } : {}),
        ...(p.homepage ? { externalReferences: [{ type: "website", url: p.homepage }] } : {}),
      })),
    ),
  )
  .sort((a, b) => a.purl.localeCompare(b.purl));

const bom = {
  bomFormat: "CycloneDX",
  specVersion: "1.5",
  serialNumber: `urn:uuid:${randomUUID()}`,
  version: 1,
  metadata: {
    timestamp: new Date().toISOString(),
    tools: { components: [{ type: "application", name: "tandem gen-sbom-npm", version: app.version }] },
    component: {
      type: "application",
      "bom-ref": "tandem-desktop",
      name: "Tandem desktop (webview)",
      version: app.version,
    },
  },
  components,
};
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, JSON.stringify(bom, null, 2) + "\n");
console.log(`npm SBOM: ${components.length} components → ${out}`);
