import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { $, browser } from "@wdio/globals";
import { Key } from "webdriverio";
import { freshOffline, home, mod } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";

// E2E-M15-3 Data controls (F-TL-05, UI-112): Settings › Privacy › "Export my
// data…" writes a zip with teammates.json, threads.json and the memory
// folders and nothing from provider homes; "Delete local data…" asks for
// "delete", keeps engine sign-ins unless ticked, and leaves the request for
// the next start. The suite's data folder is shared by every spec, so the
// delete itself (unit-tested in services/data_controls.rs) never runs here:
// the spec removes the request before anything relaunches.

const marker = () => join(home(), ".delete-pending");
const savePath = () => join(home(), "save-path.txt");
const zipPath = `/tmp/tandem-e2e-export-${Date.now()}.zip`;

/** The zip's entry names (python3's zipfile; the suite has no zip reader). */
function entries(path: string): string[] {
  const out = execFileSync("python3", ["-c", "import sys,zipfile;print('\\n'.join(zipfile.ZipFile(sys.argv[1]).namelist()))", path]);
  return out.toString().split("\n").filter(Boolean);
}

async function openPrivacy(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys([mod(), ","]);
  const nav = $('//nav[@aria-label="Settings pages"]//button[normalize-space()="Privacy & Telemetry"]');
  await nav.waitForClickable({ timeout: 10_000 });
  await nav.click();
  await $('[data-testid="data-controls"]').waitForDisplayed({ timeout: 10_000 });
}

describe("E2E-M15-3 data controls", () => {
  const memory = join(home(), "teammates", "tm_e2e", "memory");
  const providerSecret = join(home(), "providers", "claude", "e2e-export", ".credentials.json");

  before(async function () {
    this.timeout(60_000);
    await freshOffline();
    mkdirSync(memory, { recursive: true });
    writeFileSync(join(memory, "notes.md"), "Prefers small pull requests.");
    mkdirSync(join(providerSecret, ".."), { recursive: true });
    writeFileSync(providerSecret, "oauth-e2e-PROVIDER-TOKEN");
  });

  after(() => {
    // Never leave a delete request behind for a later relaunch.
    rmSync(marker(), { force: true });
    rmSync(savePath(), { force: true });
    rmSync(zipPath, { force: true });
    rmSync(join(home(), "teammates", "tm_e2e"), { recursive: true, force: true });
    rmSync(join(home(), "providers", "claude", "e2e-export"), { recursive: true, force: true });
  });

  it("F-TL-05.1 exports a zip with teammates.json, threads.json and memory, and no engine sign-ins", async () => {
    writeFileSync(savePath(), zipPath);
    await openPrivacy();
    await expectNoSeriousA11yViolationsInBothThemes();
    await $('[data-testid="data-export"]').click();
    await $("//*[normalize-space()='Exported your data']").waitForDisplayed({ timeout: 30_000 });
    expect(existsSync(zipPath)).toBe(true);
    const names = entries(zipPath);
    for (const expected of ["teammates.json", "threads.json", "settings.json", "teammates/tm_e2e/memory/notes.md", "manifest.json"]) {
      expect(names).toContain(expected);
    }
    expect(names.some((n) => n.startsWith("providers/") || n.startsWith("auth/"))).toBe(false);
    expect(readFileSync(zipPath).includes("oauth-e2e-PROVIDER-TOKEN")).toBe(false);
  });

  it("F-TL-05.2 Delete local data needs 'delete', keeps engine sign-ins by default and leaves the request for the next start", async () => {
    rmSync(marker(), { force: true });
    await $('[data-testid="data-delete"]').click();
    const dialog = $('[role="alertdialog"]');
    await dialog.waitForDisplayed({ timeout: 10_000 });
    expect(await dialog.getText()).toContain("Delete all Vibezeo data on this device?");
    await expectNoSeriousA11yViolationsInBothThemes();
    const box = dialog.$('[role="checkbox"]');
    expect(await box.getAttribute("aria-checked")).toBe("false");
    const confirm = dialog.$("button=Delete data");
    expect(await confirm.isEnabled()).toBe(false);
    await dialog.$("input").setValue("delete");
    await confirm.click();
    await browser.waitUntil(() => existsSync(marker()), { timeout: 10_000, timeoutMsg: "no delete request was left" });
    expect(JSON.parse(readFileSync(marker(), "utf8"))).toEqual({ removeEngineSignIns: false });
    rmSync(marker(), { force: true });
    await browser.keys(Key.Escape);
  });
});
