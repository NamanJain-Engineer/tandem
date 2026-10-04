import { spawn } from "node:child_process";

const api = spawn("pnpm", ["--filter", "api", "dev"], {
  stdio: "inherit",
  env: process.env,
});

const shutdown = () => {
  api.kill("SIGINT");
};

process.on("SIGINT", () => {
  shutdown();
  process.exit(0);
});
process.on("SIGTERM", () => {
  shutdown();
  process.exit(0);
});

api.on("exit", (code) => process.exit(code ?? 0));
