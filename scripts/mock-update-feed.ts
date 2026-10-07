#!/usr/bin/env tsx
/**
 * A local update feed for the updater tests (F-SH-06, MILESTONES M5).
 *
 *   pnpm tsx scripts/mock-update-feed.ts [--port 8790] [--scenario valid|badsig|lower|none]
 *
 * Serves the dynamic Tauri feed the app asks for,
 * `GET /v1/updates/:channel/:target/:arch/:current_version`, and the artifact
 * it points at. Releases are signed with the dev updater key (`uk-1` in
 * `.dev-keys/keys.json`, from `pnpm keys:dev`):
 * - `valid`: 9.9.0, correctly signed;
 * - `badsig`: 9.9.0, signed over different bytes (the app must refuse it);
 * - `lower`: 0.0.1, correctly signed (anti-rollback: the app must ignore it);
 * - `none`: 204, nothing new.
 * `POST /__scenario {"scenario": "..."}` switches scenario; `GET /__requests`
 * lists the feed requests seen (channel and current version).
 */
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { signature } from "./lib/minisign";

const root = resolve(import.meta.dirname, "..");
const args = process.argv.slice(2);
const option = (name: string, fallback: string) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? (args[i + 1] as string) : fallback;
};
const port = Number(option("port", "8790"));
type Scenario = "valid" | "badsig" | "lower" | "none";
const SCENARIOS: Scenario[] = ["valid", "badsig", "lower", "none"];
let scenario = option("scenario", "valid") as Scenario;
if (!SCENARIOS.includes(scenario)) throw new Error(`unknown scenario ${scenario}`);

const keys = JSON.parse(readFileSync(join(root, ".dev-keys/keys.json"), "utf8")) as Record<string, string>;
const key = keys["uk-1"];
if (!key) throw new Error("no dev updater key: run `pnpm keys:dev`");

/** The "update": any bytes do, the app's test installer never runs them. */
const artifact = Buffer.from(`tandem test update ${"x".repeat(64 * 1024)}`);
const requests: Array<{ channel: string; target: string; arch: string; current: string; scenario: Scenario }> = [];

function release(s: Scenario): object | null {
  if (s === "none") return null;
  const version = s === "lower" ? "0.0.1" : "9.9.0";
  const signed = s === "badsig" ? Buffer.concat([artifact, Buffer.from("tampered")]) : artifact;
  return {
    version,
    notes: `Tandem ${version} (test feed)`,
    pub_date: new Date().toISOString(),
    url: `http://127.0.0.1:${port}/artifacts/tandem-${version}.AppImage`,
    signature: signature(signed, key as string, version),
  };
}

const server = createServer((req, res) => {
  const url = new URL(req.url ?? "/", `http://127.0.0.1:${port}`);
  const feed = /^\/v1\/updates\/([^/]+)\/([^/]+)\/([^/]+)\/([^/]+)$/.exec(url.pathname);
  if (req.method === "GET" && feed) {
    const [, channel = "", target = "", arch = "", current = ""] = feed;
    requests.push({ channel, target, arch, current, scenario });
    const body = release(scenario);
    if (!body) {
      res.writeHead(204).end();
      return;
    }
    res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(body));
    return;
  }
  if (req.method === "GET" && url.pathname.startsWith("/artifacts/")) {
    res.writeHead(200, { "content-type": "application/octet-stream", "content-length": artifact.length }).end(artifact);
    return;
  }
  if (req.method === "POST" && url.pathname === "/__scenario") {
    let raw = "";
    req.on("data", (c: Buffer) => (raw += c.toString()));
    req.on("end", () => {
      const next = (JSON.parse(raw || "{}") as { scenario?: Scenario }).scenario;
      if (!next || !SCENARIOS.includes(next)) {
        res.writeHead(400).end("unknown scenario");
        return;
      }
      scenario = next;
      res.writeHead(204).end();
    });
    return;
  }
  if (req.method === "GET" && url.pathname === "/__requests") {
    res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(requests));
    return;
  }
  if (req.method === "GET" && url.pathname === "/health") {
    res.writeHead(200).end("ok");
    return;
  }
  res.writeHead(404).end();
});

server.listen(port, "127.0.0.1", () => console.log(`mock update feed on http://127.0.0.1:${port} (scenario ${scenario})`));
for (const sig of ["SIGINT", "SIGTERM"] as const) process.on(sig, () => server.close(() => process.exit(0)));
