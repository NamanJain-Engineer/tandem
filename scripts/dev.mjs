import { spawn } from "node:child_process";

const children = ["api", "desktop"].map((name) =>
  spawn("pnpm", ["--filter", name, "dev"], { stdio: "inherit", env: process.env }),
);

let stopping = false;

function stop(code) {
  if (stopping) return;
  stopping = true;
  for (const child of children) child.kill("SIGINT");
  setTimeout(() => process.exit(code), 200);
}

for (const child of children) {
  child.on("exit", (code) => {
    if (!stopping) stop(code ?? 0);
  });
}

process.on("SIGINT", () => stop(0));
process.on("SIGTERM", () => stop(0));
