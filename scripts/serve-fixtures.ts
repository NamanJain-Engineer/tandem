#!/usr/bin/env tsx
/**
 * Fixture pages for the browser tests (F-CD-17, F-CD-18; MILESTONES M9).
 *
 *   pnpm tsx scripts/serve-fixtures.ts [--port 5173]
 *
 * Serves on localhost (127.0.0.1 and ::1):
 * - `/`: "Fixtures", a plain page with links;
 * - `/probe`: looks for any way back into the app (`window.__TAURI__`, the
 *   IPC internals, the webview's message handler, the app's own schemes),
 *   names itself after what it found and posts the details to `/__report`;
 * - `/to-tandem`: sends itself to `tandem://open` as soon as it loads (a
 *   page-made navigation the browser must refuse);
 * - `/health`.
 * `GET /__reports` lists the probe reports, `GET /__requests` the pages
 * asked for, and `POST /__reset` forgets both.
 */
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";

const args = process.argv.slice(2);
const option = (name: string, fallback: string) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] ? (args[i + 1] as string) : fallback;
};
const port = Number(option("port", "5173"));

const page = (title: string, body: string) =>
  `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>${title}</title>` +
  `<style>body{font:15px/1.5 system-ui,sans-serif;margin:2rem;color:#1e293b}a{color:#4f46e5}</style></head>` +
  `<body>${body}</body></html>`;

const HOME = page(
  "Fixtures",
  `<h1>Fixtures</h1><p>Test pages for the app's browser.</p>` +
    `<ul><li><a href="/probe">Probe for the app</a></li><li><a href="/to-tandem">A page that opens tandem:</a></li></ul>`,
);

/** Every way back into the app a page might try; all must come up empty. */
const PROBE = page(
  "Probe",
  `<h1>Probe</h1><pre id="out">probing…</pre><script>
(async () => {
  const w = window;
  const report = {
    tauri: typeof w.__TAURI__,
    internals: typeof w.__TAURI_INTERNALS__,
    handler: typeof (w.webkit && w.webkit.messageHandlers && w.webkit.messageHandlers.ipc),
    post: (() => { try { w.ipc.postMessage("probe"); return "sent"; } catch (e) { return "failed"; } })(),
  };
  for (const url of ["ipc://localhost/app_boot", "tauri://localhost/"]) {
    try { await fetch(url, { method: "POST", body: "{}" }); report[url] = "answered"; } catch (e) { report[url] = "failed"; }
  }
  const found = report.tauri !== "undefined" || report.internals !== "undefined" || report.handler !== "undefined" ||
    report.post !== "failed" || Object.keys(report).some((k) => k.includes("://") && report[k] !== "failed");
  document.getElementById("out").textContent = JSON.stringify(report, null, 2);
  await fetch("/__report", { method: "POST", body: JSON.stringify(report) });
  document.title = found ? "Probe: bridge found" : "Probe: no app bridge";
})();
</script>`,
);

const TO_TANDEM = page("To tandem", `<h1>To tandem</h1><script>location.href = "tandem://open?from=page";</script>`);

const reports: unknown[] = [];
const requests: string[] = [];

function send(res: ServerResponse, status: number, type: string, body: string) {
  res.writeHead(status, { "content-type": type, "cache-control": "no-store" });
  res.end(body);
}

async function read(req: IncomingMessage): Promise<string> {
  let body = "";
  for await (const chunk of req) body += chunk;
  return body;
}

async function handle(req: IncomingMessage, res: ServerResponse) {
  const path = new URL(req.url ?? "/", "http://localhost").pathname;
  if (!path.startsWith("/__") && path !== "/health" && path !== "/favicon.ico") requests.push(path);
  if (req.method === "POST" && path === "/__report") {
    try {
      reports.push(JSON.parse(await read(req)));
    } catch {
      return send(res, 400, "text/plain", "bad report");
    }
    return send(res, 204, "text/plain", "");
  }
  if (req.method === "POST" && path === "/__reset") {
    reports.length = 0;
    requests.length = 0;
    return send(res, 204, "text/plain", "");
  }
  if (path === "/__reports") return send(res, 200, "application/json", JSON.stringify(reports));
  if (path === "/__requests") return send(res, 200, "application/json", JSON.stringify(requests));
  if (path === "/health") return send(res, 200, "text/plain", "ok");
  if (path === "/") return send(res, 200, "text/html; charset=utf-8", HOME);
  if (path === "/probe") return send(res, 200, "text/html; charset=utf-8", PROBE);
  if (path === "/to-tandem") return send(res, 200, "text/html; charset=utf-8", TO_TANDEM);
  return send(res, 404, "text/html; charset=utf-8", page("Not found", "<h1>Not found</h1>"));
}

const listener = (req: IncomingMessage, res: ServerResponse) => {
  handle(req, res).catch(() => send(res, 500, "text/plain", "error"));
};

// Both loopback addresses: `localhost` may resolve to either.
createServer(listener).listen(port, "127.0.0.1", () => console.log(`fixtures on http://localhost:${port}`));
createServer(listener)
  .on("error", () => undefined)
  .listen(port, "::1");
