// Renders preview stills: node stills.mjs 30 160 300 ...
import { bundle } from "@remotion/bundler";
import { renderStill, selectComposition } from "@remotion/renderer";
import { mkdirSync } from "node:fs";
import path from "node:path";

const browserExecutable = process.env.BROWSER;
const serveUrl = await bundle({ entryPoint: path.resolve("src/index.ts"), publicDir: path.resolve("public") });
const composition = await selectComposition({ serveUrl, id: "Product", browserExecutable });
mkdirSync("stills", { recursive: true });
for (const f of process.argv.slice(2).map(Number)) {
  await renderStill({ composition, serveUrl, frame: f, output: `stills/f${String(f).padStart(4, "0")}.png`, browserExecutable, scale: 0.5 });
  console.log("still", f);
}
