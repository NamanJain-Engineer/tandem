// Renders the full product video: node render.mjs out/vibezeo-product.mp4
import { bundle } from "@remotion/bundler";
import { renderMedia, selectComposition } from "@remotion/renderer";
import path from "node:path";

const browserExecutable = process.env.BROWSER;
const out = process.argv[2] ?? "out/vibezeo-product.mp4";
const serveUrl = await bundle({ entryPoint: path.resolve("src/index.ts"), publicDir: path.resolve("public") });
const composition = await selectComposition({ serveUrl, id: "Product", browserExecutable });
let last = -1;
await renderMedia({
  composition,
  serveUrl,
  codec: "h264",
  crf: 16,
  audioCodec: "aac",
  audioBitrate: "256k",
  outputLocation: out,
  browserExecutable,
  concurrency: 2,
  offthreadVideoCacheSizeInBytes: 512 * 1024 * 1024,
  timeoutInMilliseconds: 120000,
  onProgress: ({ progress }) => {
    const p = Math.floor(progress * 10);
    if (p !== last) {
      last = p;
      console.log(`progress ${p * 10}%`);
    }
  },
});
console.log("done", out);
