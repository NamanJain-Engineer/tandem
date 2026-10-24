import { $, browser } from "@wdio/globals";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { freshUser, home, invoke, mod } from "../support/app";
import { lead } from "../support/agents";

type Status = { installed: boolean; kind: string; path: string | null };

const status = () => invoke<Status>("routines_background_status");
const systemctlLog = () => {
  const f = join(home(), "systemctl.log");
  return existsSync(f) ? readFileSync(f, "utf8") : "";
};

// E2E-M11-5 Background (F-RT-03): turning background runs on installs the
// systemd user timer (under TANDEM_LAUNCHER_ROOT, with a recording
// `systemctl`), turning them off removes it, and `--run-routines` started
// while the GUI runs exits at once without running anything.
describe("E2E-M11-5 background runner", () => {
  before(async function () {
    this.timeout(180_000);
    await freshUser("m11-5");
    await browser.keys([mod(), "1"]);
    await $('[data-testid="agent-footer-routines"]').click();
    await $('[data-testid="routines-page"]').waitForDisplayed({ timeout: 10_000 });
  });

  it("F-RT-03.1 Turn on installs the unit files; Turn off removes them", async function () {
    this.timeout(60_000);
    expect((await status()).installed).toBe(false);
    await $('[data-testid="routines-background-on"]').click();
    await $('[data-testid="routines-background"][data-on="true"]').waitForDisplayed({ timeout: 10_000 });
    const on = await status();
    expect(on.installed).toBe(true);
    expect(on.kind).toBe("systemd");
    const timer = on.path!;
    expect(timer.startsWith(process.env.TANDEM_LAUNCHER_ROOT!)).toBe(true);
    expect(existsSync(timer)).toBe(true);
    const service = timer.replace(/\.timer$/, ".service");
    expect(existsSync(service)).toBe(true);
    expect(readFileSync(service, "utf8")).toContain("--run-routines");
    expect(systemctlLog()).toMatch(/--user enable --now \S+\.timer/);
    await $('[data-testid="routines-background-off"]').click();
    await $('[data-testid="routines-background"][data-on="false"]').waitForDisplayed({ timeout: 10_000 });
    expect(existsSync(timer)).toBe(false);
    expect(existsSync(service)).toBe(false);
    expect((await status()).installed).toBe(false);
    expect(systemctlLog()).toMatch(/--user disable --now \S+\.timer/);
  });

  it("F-RT-03.2 with the GUI running, --run-routines exits without running anything", async function () {
    this.timeout(60_000);
    const r = await invoke<{ id: string; nextRunAt: number }>("routine_save", {
      input: {
        teammateId: (await lead()).id,
        name: "Background digest",
        instruction: "Summarize what changed overnight in three bullets.",
        schedule: { schedule: { kind: "cron", expr: "0 4 * * *" }, timezone: "UTC" },
        access: "edits",
        fullConfirmed: false,
        runMode: "background",
      },
    });
    // The GUI holds the app lock.
    const info = readFileSync(join(process.env.TANDEM_HOME!, "run", "app.lock.json"), "utf8");
    expect(info).toContain("gui");
    // A headless pass whose clock says the routine is an hour overdue.
    const started = Date.now();
    const out = spawnSync(process.env.TANDEM_APP_BINARY!, ["--run-routines"], {
      env: {
        ...process.env,
        PATH: process.env.TANDEM_APP_PATH ?? process.env.PATH,
        TANDEM_FAKE_CLOCK: new Date(r.nextRunAt + 3_600_000).toISOString(),
      },
      timeout: 30_000,
    });
    expect(out.status).toBe(0);
    expect(Date.now() - started).toBeLessThan(15_000);
    const runs = (await invoke<{ runs: unknown[] }>("routine_runs", { id: r.id, before: null })).runs;
    expect(runs).toHaveLength(0);
    // The launcher was installed for the background routine; leave it off.
    await invoke("routines_background_set", { enabled: false });
  });
});
