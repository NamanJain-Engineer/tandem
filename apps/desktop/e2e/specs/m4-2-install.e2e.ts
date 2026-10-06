import { $, browser } from "@wdio/globals";
import { chmodSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, checkAgain, freshOffline, home, paneIds, paneText, setRegistryOverrides, textOf, userHome } from "../support/app";

// E2E-M4-2 Install (F-EN-03): a registry override points gemini's installer
// at a fake script that puts a fake `gemini` in ~/.local/bin.
describe("E2E-M4-2 install", () => {
  const script = () => join(home(), "install-gemini.sh");

  after(async () => {
    await setRegistryOverrides([]);
    rmSync(join(userHome(), ".local/bin/gemini"), { force: true });
    await checkAgain().catch(() => undefined);
  });

  it("F-EN-03.1 the sheet shows the exact command; Run executes it in a visible pane; then Installed ✓", async () => {
    writeFileSync(
      script(),
      `#!/bin/sh\nset -e\nmkdir -p "$HOME/.local/bin"\nln -sf "${process.env.TANDEM_FAKE_ENGINE_BIN}" "$HOME/.local/bin/gemini"\necho "gemini installed"\n`,
    );
    chmodSync(script(), 0o755);
    const command = `sh ${script()}`;
    await setRegistryOverrides([{ id: "gemini", install: { mac: command, lin: command, win: command } }]);
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m4-2-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    // Check again re-reads the remote registry.
    await checkAgain();
    const [first] = await paneIds();
    await $(`[data-testid="pane"][data-pane-id="${first}"] button[aria-label^="Close "]`).click();
    await $('[data-testid="more-engines"]').waitForClickable({ timeout: 10_000 });
    await $('[data-testid="more-engines"]').click();
    await $('[data-testid="install-gemini"]').click();
    await $('[data-testid="install-command"]').waitForExist({ timeout: 10_000 });
    // In-page text: WebKitWebDriver's getText reads "" inside the opening dialog.
    expect(await textOf('[data-testid="install-command"]')).toBe(command);
    await $('[data-testid="install-run"]').click();
    // A visible terminal pane runs it.
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 15_000 });
    const [pane] = await paneIds();
    await browser.waitUntil(async () => (await paneText(pane ?? "")).includes("gemini installed"), { timeout: 20_000 });
    await $('[data-testid="install-done"]').waitForExist({ timeout: 20_000 });
    expect(await textOf('[data-testid="install-sheet"]')).toContain("Installed ✓ — Sign in next");
  });
});
