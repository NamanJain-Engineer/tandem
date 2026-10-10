import { readFileSync } from "node:fs";
import { join } from "node:path";
import { DOWNLOADS, assetUrl } from "./release";
import { SITE } from "./site";

/** The release manifest: `SHA256SUMS.txt` beside the builds in
 * `public/downloads`, as `release-local.sh` writes it (`sha256sum` lines).
 * Read at build time, so the site shows exactly the hashes it serves
 * (F-WB-04). */
export function releaseChecksums(dir = join(process.cwd(), "public/downloads")): Map<string, string> {
  const sums = new Map<string, string>();
  for (const line of readFileSync(join(dir, "SHA256SUMS.txt"), "utf8").split("\n")) {
    const m = /^([0-9a-f]{64}) [ *](.+)$/.exec(line.trim());
    if (m?.[1] && m[2]) sums.set(m[2], m[1]);
  }
  return sums;
}

export type ReleaseAsset = { file: string; label: string; url: string; sha256: string };

/** The available builds with their SHA-256. A build missing from the
 * manifest fails the site build: a download is never shown unchecked. */
export function releaseAssets(sums = releaseChecksums()): Map<string, ReleaseAsset[]> {
  const out = new Map<string, ReleaseAsset[]>();
  for (const d of DOWNLOADS.filter((d) => d.available)) {
    out.set(
      d.os,
      d.assets.map((a) => {
        const sha256 = sums.get(a.file);
        if (!sha256) throw new Error(`${a.file} is not in public/downloads/SHA256SUMS.txt`);
        return { ...a, url: `${SITE.url}${assetUrl(a.file)}`, sha256 };
      }),
    );
  }
  return out;
}
