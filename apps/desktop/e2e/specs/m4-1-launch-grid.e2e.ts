import { $, $$, browser } from "@wdio/globals";
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  checkAgain,
  fakeEngine,
  freshOffline,
  paneIds,
  profileWithoutEngines,
  setProfile,
  userHome,
} from "../support/app";

// E2E-M4-1 Launch grid (F-CD-13, F-EN-01).
describe("E2E-M4-1 launch grid", () => {
  const nvmBin = () => join(userHome(), ".nvm/versions/node/v20.11.1/bin");

  after(() => {
    rmSync(join(userHome(), ".nvm"), { recursive: true, force: true });
    rmSync(join(userHome(), ".zshrc"), { force: true });
    setProfile(null);
  });

  /** The empty tab: close the tab's shell so the launch grid shows. */
  async function emptyTab(): Promise<void> {
    const [pane] = await paneIds();
    await $(`[data-testid="pane"][data-pane-id="${pane}"] button[aria-label^="Close "]`).click();
    await $('[data-testid="launch-grid"]').waitForExist({ timeout: 10_000 });
  }

  it("F-EN-01.1 F-CD-13.1 claude only in an nvm folder is found and comes first; the rest are not installed", async () => {
    // Only an nvm-style folder that no PATH has holds `claude`.
    setProfile(profileWithoutEngines());
    fakeEngine("claude", nvmBin());
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m4-1-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const report = await checkAgain();
    expect(report.found.map((f) => [f.engineId, f.source])).toEqual([["claude", "known"]]);
    await emptyTab();
    await browser.waitUntil(
      async () => (await $$('[data-testid="launch-grid"] [data-testid^="tile-"]').length) === 2,
      { timeout: 10_000 },
    );
    const tiles = await browser.execute(() =>
      Array.from(
        document.querySelectorAll<HTMLElement>('[data-testid="launch-grid"] [data-testid^="tile-"]'),
      ).map((el) => el.dataset.testid),
    );
    expect(tiles).toEqual(["tile-claude", "tile-shell"]);
    expect(await $('[data-testid="more-engines"]').getText()).toContain("12 more not installed");
    // Launching it runs the engine found in the nvm folder.
    await $('[data-testid="tile-claude"]').click();
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 15_000 });
  });

  it("F-EN-01 F-CD-13.2 with nothing installed the grid lists the shells probed, and Check again detects again", async () => {
    rmSync(join(userHome(), ".nvm"), { recursive: true, force: true });
    await checkAgain();
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m4-1b-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    await emptyTab();
    const diag = await $('[data-testid="engines-diagnostics"]');
    await diag.waitForExist({ timeout: 10_000 });
    expect(await $('[data-testid="shells-asked"]').getText()).toMatch(/-lic/);
    // Installed meanwhile: Check again finds it.
    fakeEngine("claude", nvmBin());
    await $('[data-testid="check-again"]').click();
    await $('[data-testid="launch-grid"] [data-testid="tile-claude"]').waitForExist({ timeout: 20_000 });
  });

  it("F-EN-01.2 a shell rc that sleeps 20 s gives the slow-shell diagnostic at 8 s and known folders still answer", async () => {
    writeFileSync(join(userHome(), ".zshrc"), "sleep 20\n");
    const started = Date.now();
    const report = await checkAgain();
    const took = Date.now() - started;
    expect(report.slowShell).not.toBeNull();
    expect(report.found.map((f) => f.engineId)).toContain("claude");
    // 8 s for the probe (plus the 3 s login-env capture that also times out).
    expect(took).toBeGreaterThanOrEqual(8_000);
    expect(took).toBeLessThan(20_000);
    await $('[data-testid="slow-shell"]').waitForExist({ timeout: 10_000 });
    expect(await $('[data-testid="slow-shell"]').getText()).toBe("Your shell took more than 8s to start");
    rmSync(join(userHome(), ".zshrc"), { force: true });
  });
});
