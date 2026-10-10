// Serves Dictate models from the local cache (~/.cache/tandem-models, or
// $TANDEM_MODEL_CACHE) at `/<model-id>/<file>`, the layout the app asks for
// when TANDEM_MODELS_BASE points here (debug builds). Ranges are honoured.
//
//   tsx scripts/serve-models.ts --port 8791 [--corrupt-byte 1000] [--control <file>]
//
// `--corrupt-byte N` flips byte N of every file it serves (E2E-M8-8: the
// SHA-256 check must fail). `--control <file>` re-reads that file on each
// request: a number there corrupts that byte, "slow" sends 64 KB a second,
// anything else (or no file) serves the bytes as they are.
import { createReadStream, existsSync, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { homedir } from "node:os";
import { join, normalize } from "node:path";
import { Transform } from "node:stream";

const args = process.argv.slice(2);
const arg = (name: string) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};
const port = Number(arg("--port") ?? 8791);
const fixedCorrupt = arg("--corrupt-byte");
const control = arg("--control");
const cache = process.env.TANDEM_MODEL_CACHE ?? join(homedir(), ".cache", "tandem-models");

function mode(): { corrupt?: number; slow?: boolean } {
  if (fixedCorrupt !== undefined) return { corrupt: Number(fixedCorrupt) };
  if (!control || !existsSync(control)) return {};
  const raw = readFileSync(control, "utf8").trim();
  if (raw === "slow") return { slow: true };
  const n = Number(raw);
  return Number.isFinite(n) && raw !== "" ? { corrupt: n } : {};
}

createServer((req, res) => {
  if (req.url === "/health") {
    res.writeHead(200).end("ok");
    return;
  }
  const parts = (req.url ?? "/").split("?")[0].split("/").filter(Boolean);
  if (parts.length !== 2 || parts.some((p) => p.includes("..") || p.startsWith("."))) {
    res.writeHead(404).end();
    return;
  }
  const path = normalize(join(cache, parts[0], parts[1]));
  if (!path.startsWith(cache) || !existsSync(path)) {
    res.writeHead(404).end();
    return;
  }
  const size = statSync(path).size;
  const m = mode();
  let start = 0;
  const range = /^bytes=(\d+)-$/.exec(req.headers.range ?? "");
  if (range) start = Number(range[1]);
  if (start >= size) {
    res.writeHead(416).end();
    return;
  }
  res.writeHead(range ? 206 : 200, {
    "content-length": String(size - start),
    "content-type": "application/octet-stream",
    ...(range ? { "content-range": `bytes ${start}-${size - 1}/${size}` } : {}),
  });
  let offset = start;
  const flip = new Transform({
    transform(chunk: Buffer, _enc, done) {
      if (m.corrupt !== undefined && m.corrupt >= offset && m.corrupt < offset + chunk.length) {
        chunk = Buffer.from(chunk);
        chunk[m.corrupt - offset] ^= 0xff;
      }
      offset += chunk.length;
      if (m.slow) setTimeout(() => done(null, chunk), (chunk.length / 65_536) * 1000);
      else done(null, chunk);
    },
  });
  createReadStream(path, { start, highWaterMark: m.slow ? 16_384 : 1 << 20 })
    .pipe(flip)
    .pipe(res);
}).listen(port, "127.0.0.1", () => {
  console.log(`serve-models: ${cache} on http://127.0.0.1:${port}`);
});
