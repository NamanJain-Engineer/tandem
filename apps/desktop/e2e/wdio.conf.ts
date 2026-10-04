import { spawn, type ChildProcess } from "node:child_process";
import { join } from "node:path";

const desktop = new URL("..", import.meta.url).pathname;
let driver: ChildProcess | undefined;

export const config: WebdriverIO.Config = {
  runner: "local",
  hostname: "127.0.0.1",
  port: 4444,
  specs: [join(desktop, "e2e/specs/**/*.e2e.ts")],
  maxInstances: 1,
  capabilities: [
    {
      // @ts-expect-error tauri-driver specific capability
      "tauri:options": { application: process.env.TANDEM_APP_BINARY },
      // WebKitWebDriver speaks classic WebDriver only (no BiDi).
      "wdio:enforceWebDriverClassic": true,
    },
  ],
  logLevel: "warn",
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: { ui: "bdd", timeout: 60_000 },
  onPrepare: () =>
    new Promise<void>((resolve) => {
      driver = spawn("tauri-driver", [], { stdio: [null, process.stdout, process.stderr] });
      setTimeout(resolve, 1500);
    }),
  onComplete: () => {
    driver?.kill();
  },
};
