import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import hosted from "@tandem/protocol/hosted-features.json";
import subprocessors from "@tandem/protocol/subprocessors.json";
import { CREDIT_PRICES, PRICES } from "@tandem/protocol/plans";
import { CREDIT_ROWS, PLANS } from "../lib/pricing";
import { CHECKSUMS_URL, DOWNLOADS, assetUrl } from "../lib/release";
import { SITE } from "../lib/site";
import DownloadPage from "./download/page.site";
import PricingPage from "./pricing/page.site";
import PrivacyPage from "./privacy/page.site";

const html = (el: ReactElement) => renderToStaticMarkup(el);
const pub = (p: string) => join(import.meta.dirname, "../public", p);

describe("F-WB-01 marketing site (M15)", () => {
  it("F-WB-01 AC3 the pricing page's prices match plans.ts and the API's signed remote config", () => {
    const dollars = (cents: number) => `$${cents / 100}`;
    const plan = (id: string) => PLANS.find((p) => p.id === id);
    expect(plan("pro")?.price).toBe(dollars(PRICES.pro.month));
    expect(plan("pro")?.cadence).toContain(`${dollars(PRICES.pro.year)} a year`);
    expect(plan("team")?.price).toBe(dollars(PRICES.team.month));
    expect(plan("team")?.cadence).toContain(`${dollars(PRICES.team.year)} a year`);
    expect(plan("team")?.cadence).toContain(`${PRICES.team.minSeats} seats minimum`);
    const remote = JSON.parse(
      readFileSync(join(import.meta.dirname, "../../api/config/remote-config.json"), "utf8"),
    ) as { prices: Record<string, number> };
    const page = html(<PricingPage />);
    expect(CREDIT_ROWS.length).toBeGreaterThan(0);
    for (const row of CREDIT_ROWS) {
      expect(remote.prices[row.key], row.key).toBe(CREDIT_PRICES[row.key]);
      expect(row.credits, row.key).toBe(remote.prices[row.key]);
      expect(page).toContain(`data-credit-price="${row.key}"`);
    }
  });

  it("F-WB-01 /pricing prices only hosted features that are built (ticked in REMAINING)", () => {
    const remaining = readFileSync(join(import.meta.dirname, "../../../docs/REMAINING.md"), "utf8");
    const page = html(<PricingPage />);
    for (const row of CREDIT_ROWS) expect(remaining, row.feature).toContain(`- [x] ${row.feature} `);
    // F-DI-10 is open: Enhance and Polish isn't in the app, so it isn't priced or named.
    expect(remaining).toContain("- [ ] F-DI-10 ");
    expect(page).not.toContain('data-credit-price="rewrite"');
    expect(page).not.toMatch(/Enhance|Polish/);
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
