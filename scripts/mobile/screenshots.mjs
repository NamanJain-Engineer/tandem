// Store-preview screenshots of Vibezeo Remote's demo mode, rendered with
// react-native-web in Chromium (this container has no Android emulator).
// They show the same components; replace them with captures from real
// devices before publishing (HUMAN_TODO).
//   pnpm --filter mobile preview:web && node scripts/mobile/screenshots.mjs [outdir]
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { mkdirSync } from "node:fs";
import { extname, join, resolve } from "node:path";
import { createRequire } from "node:module";

const require = createRequire(resolve("apps/web/package.json"));
const { chromium } = require("@playwright/test");
const root = resolve("apps/mobile/dist/web");
const out = resolve(process.argv[2] ?? "dist/mobile/screenshots");
mkdirSync(out, { recursive: true });
const types = { ".html": "text/html", ".js": "text/javascript", ".png": "image/png", ".ttf": "font/ttf", ".json": "application/json" };
const server = createServer(async (req, res) => {
  const path = decodeURIComponent((req.url ?? "/").split("?")[0]);
  try {
    const file = join(root, path === "/" ? "index.html" : path);
    const body = await readFile(file);
    res.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream" });
    res.end(body);
  } catch {
    res.writeHead(200, { "content-type": "text/html" });
    res.end(await readFile(join(root, "index.html")));
  }
}).listen(0, "127.0.0.1");
await new Promise((r) => server.once("listening", r));
const url = `http://127.0.0.1:${server.address().port}/`;
const browser = await chromium.launch({ executablePath: process.env.PLAYWRIGHT_CHROMIUM ?? "/opt/pw-browsers/chromium" });
const errors = [];

async function shoot(name, viewport, steps) {
  const page = await browser.newPage({ viewport, deviceScaleFactor: 2 });
  page.on("pageerror", (e) => errors.push(`${name}: ${e.message}`));
  await page.goto(url);
  await page.getByText("Try the demo").click();
  await page.getByText("Demo computer").first().waitFor();
  await page.waitForTimeout(500);
  for (const [file, act] of steps) {
    if (act) await act(page);
    await page.waitForTimeout(400);
    await page.screenshot({ path: join(out, `${name}-${file}.png`) });
  }
  await page.close();
}

const tab = (label) => (p) => p.getByRole("tab", { name: label }).click();
await shoot("phone", { width: 412, height: 892 }, [
  ["1-agents", null],
  ["2-approvals", tab("Approvals")],
  ["3-threads", tab("Threads")],
  ["4-thread", (p) => p.getByText("Fix the login redirect").first().click()],
  ["5-routines", tab("Routines")],
  ["6-settings", tab("Settings")],
]);
await shoot("tablet", { width: 1194, height: 834 }, [
  ["1-agents", null],
  ["2-thread", async (p) => { await tab("Threads")(p); await p.getByText("Ava (reviewer)").first().click(); }],
]);
await browser.close();
server.close();
if (errors.length) {
  console.error(errors.join("\n"));
  process.exit(1);
}
console.log(`screenshots in ${out}`);
