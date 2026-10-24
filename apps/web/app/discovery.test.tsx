import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import DownloadPage from "./download/page.site";
import { GET as llmsGet } from "./llms.txt/route.site";
import { GET as siteInfoGet } from "./api/v1/site-info.json/route.site";
import { GET as openApiGet } from "./openapi.json/route.site";
import { SITE_INFO_PATH, SiteInfoSchema, llmsFullTxt, llmsTxt, openApi, siteInfo } from "../lib/discovery";
import { DOCS_PAGES } from "../lib/docs/pages";
import { enginesTable, shortcutsTable, tableMarkdown } from "../lib/docs/tables";
import { releaseAssets, releaseChecksums } from "../lib/manifest";
import { DOWNLOADS, RELEASE } from "../lib/release";

describe("downloads and discovery (F-WB-04)", () => {
  it("F-WB-04.1 /llms.txt exists, says Vibezeo is free and quotes no price (D-869)", async () => {
    const res = llmsGet();
    expect(res.headers.get("content-type")).toContain("text/plain");
    const txt = await res.text();
    expect(txt).toBe(llmsTxt());
    expect(txt).toMatch(/^# Vibezeo\n\n> /);
    expect(txt).toContain("- Vibezeo is free, with no paid plans.");
    expect(txt).not.toMatch(/\$\d|\bcredits?\b|per seat|\/pricing\//);
    for (const p of DOCS_PAGES)
      expect(txt).toContain(`[${p.title}](https://vibezeo.com/docs/${p.slug ? `${p.slug}/` : ""})`);
    // Conductor needs Vibezeo's servers: DESIGN §9.5's label, never "not in this
    // release" (D-2913). Routines run, free (D-2900).
    expect(txt).toMatch(new RegExp(`/docs/conductor/\\): [^\\n]*\\(Comes with Vibezeo accounts\\.\\)`));
    expect(txt).not.toMatch(/Not in this release|In the next release/);
    for (const slug of ["thread-mode", "agent-mode", "dictate", "tasks", "plugins", "routines", "remote", "conductor"])
      expect(txt).not.toMatch(new RegExp(`/docs/${slug}/\\): [^\\n]*Not in the app yet`));
    for (const slug of ["thread-mode", "agent-mode", "dictate", "tasks", "plugins", "remote", "routines"])
      expect(txt).not.toMatch(new RegExp(`/docs/${slug}/\\): [^\\n]*Not in this release`));
  });

  it("F-WB-04 /llms-full.txt carries every docs page, with the generated tables as Markdown", () => {
    const txt = llmsFullTxt();
    expect(txt.startsWith(llmsTxt())).toBe(true);
    for (const p of DOCS_PAGES) expect(txt).toContain(`# ${p.title}\n`);
    expect(txt).toContain(tableMarkdown(shortcutsTable()));
    expect(txt).toContain(tableMarkdown(enginesTable()));
    // No limits table: nothing local is limited (D-2900).
    expect(txt).not.toContain("| Thread mode engines |");
    expect(txt).not.toMatch(/<[A-Z]\w* \/>/);
    expect(txt).not.toContain("](/");
  });

  it("F-WB-04 /api/v1/site-info is the JSON the schema describes: name, version, platforms, and that it's free", async () => {
    const res = siteInfoGet();
    expect(res.headers.get("content-type")).toBe("application/json");
    const info = SiteInfoSchema.parse(await res.json());
    expect(info).toEqual(siteInfo());
    expect(info.name).toBe("Vibezeo");
    expect(info.version).toBe(RELEASE.version);
    expect(info.platforms.map((p) => [p.os, p.available])).toEqual(DOWNLOADS.map((d) => [d.os, d.available]));
    expect(info.free).toBe(true);
    expect(info).not.toHaveProperty("pricing");
  });

  it("F-WB-04 the OpenAPI 3.1 document describes the public endpoint with the same schema", async () => {
    const doc = (await openApiGet().json()) as ReturnType<typeof openApi>;
    expect(doc).toEqual(JSON.parse(JSON.stringify(openApi())));
    expect(doc.openapi).toBe("3.1.0");
    expect(doc.servers).toEqual([{ url: "https://vibezeo.com" }]);
    expect(Object.keys(doc.paths)).toEqual([SITE_INFO_PATH]);
    const schema = doc.components.schemas.SiteInfo as {
      required: string[];
      properties: Record<string, unknown>;
    };
    expect(schema.required).toEqual(expect.arrayContaining(["name", "version", "platforms", "free"]));
    expect(Object.keys(schema.properties)).toEqual(Object.keys(siteInfo()));
  });

  // It hashes every served package (a .deb and an .rpm, about 70 MB), which
  // under the gate's parallel load can outlast vitest's 5 s default.
  it(
    "F-WB-04 each download shows the SHA-256 from the release manifest, which is the file's own",
    { timeout: 60_000 },
    () => {
      const sums = releaseChecksums();
      const page = renderToStaticMarkup(<DownloadPage />);
      for (const [, assets] of releaseAssets()) {
        for (const a of assets) {
          const file = readFileSync(join(process.cwd(), "public/downloads", a.file));
          expect(createHash("sha256").update(file).digest("hex"), a.file).toBe(sums.get(a.file));
          expect(page).toContain(`data-sha256="${a.file}"`);
          expect(page).toContain(a.sha256);
        }
      }
      expect(siteInfo().platforms.find((p) => p.os === "linux")?.downloads[0]?.sha256).toBe(
        sums.get(DOWNLOADS.find((d) => d.os === "linux")?.assets[0]?.file ?? ""),
      );
    },
  );

  it("F-WB-04 a build missing from the manifest fails the build instead of showing no checksum", () => {
    expect(() => releaseAssets(new Map())).toThrow(/not in public\/downloads\/SHA256SUMS.txt/);
  });

  it("F-WB-04 Vercel serves /api/v1/site-info from the exported JSON, readable from any origin", () => {
    const vercel = JSON.parse(readFileSync(join(process.cwd(), "vercel.json"), "utf8")) as {
      rewrites: { source: string; destination: string }[];
      headers: { source: string; headers: { key: string; value: string }[] }[];
    };
    expect(vercel.rewrites).toContainEqual({
      source: `${SITE_INFO_PATH}/`,
      destination: `${SITE_INFO_PATH}.json`,
    });
    const api = vercel.headers.find((h) => h.source === "/api/v1/(.*)");
    expect(api?.headers).toContainEqual({ key: "Access-Control-Allow-Origin", value: "*" });
  });
});
