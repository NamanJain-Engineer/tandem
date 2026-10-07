// `pnpm dev`: infra (docker compose), then the API, the web app and the desktop
// app with prefixed logs. Ctrl-C stops the apps and leaves infra running.
//   --no-desktop   skip the desktop app
//   --api-only     only the API (used by the E2E runner)
//   --no-infra     don't touch docker compose
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { lookup } from "node:dns/promises";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const args = new Set(process.argv.slice(2));
const apiOnly = args.has("--api-only");
const withDesktop = !apiOnly && !args.has("--no-desktop");
const withWeb = !apiOnly;

const colors = { infra: 90, api: 36, web: 35, desktop: 33 } as const;
function log(name: keyof typeof colors, line: string) {
  process.stdout.write(`\x1b[${colors[name]}m${name.padEnd(7)}\x1b[0m ${line}\n`);
}

async function resolveApiUrl(): Promise<string> {
  if (process.env.TANDEM_API_URL) return process.env.TANDEM_API_URL;
  try {
    await lookup("api.tandem.local");
    return "http://api.tandem.local:8787";
  } catch {
    log("infra", "api.tandem.local doesn't resolve; using 127.0.0.1 for this run.");
    log("infra", "To use the name, add to /etc/hosts:  127.0.0.1 api.tandem.local app.tandem.local");
    return "http://127.0.0.1:8787";
  }
}

function hasScript(pkg: string, script: string): boolean {
  const manifest = JSON.parse(readFileSync(join(root, pkg, "package.json"), "utf8")) as {
    scripts?: Record<string, string>;
  };
  return Boolean(manifest.scripts?.[script]);
}

const children: ChildProcess[] = [];
function run(name: keyof typeof colors, cmd: string, cmdArgs: string[], env: NodeJS.ProcessEnv = {}) {
  const child = spawn(cmd, cmdArgs, {
    cwd: root,
    env: { ...process.env, ...env },
    stdio: ["ignore", "pipe", "pipe"],
    detached: true,
  });
  children.push(child);
  for (const stream of [child.stdout, child.stderr]) {
    let buf = "";
    stream?.on("data", (chunk: Buffer) => {
      buf += chunk.toString();
      const lines = buf.split("\n");
      buf = lines.pop() ?? "";
      for (const line of lines) log(name, line);
    });
  }
  child.on("exit", (code, signal) => log(name, `exited (${signal ?? code})`));
}

function stopAll() {
  for (const child of children) {
    if (child.pid && child.exitCode === null) {
      try {
        process.kill(-child.pid, "SIGTERM");
      } catch {
        // already gone
      }
    }
  }
}

async function main() {
  if (!args.has("--no-infra")) {
    log("infra", "docker compose up -d --wait");
    const up = spawnSync("docker", ["compose", "-f", "infra/docker-compose.yml", "up", "-d", "--wait"], {
      cwd: root,
      stdio: "inherit",
    });
    if (up.status !== 0) {
      log("infra", "compose failed; is Docker running? Continue with --no-infra to skip.");
      process.exit(1);
    }
    if (hasScript("apps/api", "db:migrate")) {
      spawnSync("pnpm", ["--filter", "api", "db:migrate"], { cwd: root, stdio: "inherit" });
      spawnSync("pnpm", ["--filter", "api", "db:seed"], { cwd: root, stdio: "inherit" });
    }
  }
  const apiUrl = await resolveApiUrl();
  run("api", "pnpm", ["--filter", "api", "dev"]);
  if (withWeb)
    run("web", "pnpm", ["--filter", "web", "dev"], {
      NEXT_PUBLIC_API_URL: apiUrl,
      API_INTERNAL_URL: "http://127.0.0.1:8787",
    });
  if (withDesktop) {
    run("desktop", "pnpm", ["--filter", "desktop", "tauri", "dev"], {
      TANDEM_HOME: process.env.TANDEM_HOME ?? join(root, ".dev-data"),
      TANDEM_API_URL: apiUrl,
    });
  }
  for (const sig of ["SIGINT", "SIGTERM"] as const) {
    process.on(sig, () => {
      log("infra", "stopping apps (infra keeps running; pnpm infra:down stops it)");
      stopAll();
      setTimeout(() => process.exit(0), 1500);
    });
  }
}

void main();
