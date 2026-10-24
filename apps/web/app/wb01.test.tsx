import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import hosted from "@tandem/protocol/hosted-features.json";
import subprocessors from "@tandem/protocol/subprocessors.json";
import { CHECKSUMS_URL, DOWNLOADS, assetUrl } from "../lib/release";
import { SITE } from "../lib/site";
import DownloadPage from "./download/page.site";
import PrivacyPage from "./privacy/page.site";

const html = (el: ReactElement) => renderToStaticMarkup(el);
const pub = (p: string) => join(import.meta.dirname, "../public", p);

describe("F-WB-01 marketing site (M15)", () => {
  it("F-WB-01 AC3 Vibezeo is free (D-869): there is no pricing page, and the download page quotes no price or credit", () => {
    expect(existsSync(join(import.meta.dirname, "pricing/page.site.tsx"))).toBe(false);
    expect(existsSync(join(import.meta.dirname, "../lib/pricing.ts"))).toBe(false);
    const download = html(<DownloadPage />);
    expect(download).not.toMatch(/\$\d|\bcredits?\b|\bPro\b/);
    expect(download).toContain("Free, with no paid plans.");
  });

  it("F-WB-01 /privacy lists what the app can send, when, and who processes it", () => {
    const h = html(<PrivacyPage />);
    for (const f of hosted.features) expect(h, f.id).toContain(`data-hosted-feature="${f.id}"`);
    for (const s of subprocessors.subprocessors as Array<{ name: string }>)
      expect(h, s.name).toContain(s.name);
    // What the released download does: hosted services are off in it.
    expect(h).toContain("The download on this site runs with Vibezeo&#x27;s hosted services turned off");
  });

  it("F-WB-01 AC2 the site has no analytics, ads or cookies, so there is nothing to ask consent for (D-132)", () => {
    expect(SITE.analytics).toBeNull();
    const vercel = readFileSync(join(import.meta.dirname, "../vercel.json"), "utf8");
    expect(vercel).not.toMatch(/set-cookie/i);
    for (const script of ["demo/embed.js", "download/detect.js"]) {
      const src = readFileSync(pub(script), "utf8");
      expect(src, script).not.toMatch(
        /https?:\/\/|document\.cookie|localStorage|sessionStorage|sendBeacon|fetch\(/,
      );
    }
  });

  it("F-WB-01 /download names the visitor's OS and keeps Linux as the only button while it's the only build", () => {
    const h = html(<DownloadPage />);
    expect(h).toContain('<script src="/download/detect.js" defer="">');
    expect(h).toContain('data-os-note=""');
    const detect = readFileSync(pub("download/detect.js"), "utf8");
    for (const d of DOWNLOADS.filter((x) => !x.available)) expect(detect, d.os).toContain(d.name);
  });

  it("F-WB-04 the checksums file downloads under its own name (no trailing-slash redirect)", () => {
    expect(CHECKSUMS_URL).toBe("/downloads/SHA256SUMS.txt");
    const sums = readFileSync(pub(CHECKSUMS_URL), "utf8");
    for (const d of DOWNLOADS.filter((x) => x.available))
      for (const a of d.assets) {
        expect(existsSync(pub(assetUrl(a.file))), a.file).toBe(true);
        expect(sums, a.file).toMatch(new RegExp(`^[0-9a-f]{64}  ${a.file.replace(/\./g, "\\.")}$`, "m"));
      }
  });
});
