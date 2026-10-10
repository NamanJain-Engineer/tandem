import { $, browser } from "@wdio/globals";
import { freshOffline, invoke } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import { openPlugins } from "../support/plugins";

// E2E-M12-6 Preview (F-PL-09 AC2, F-PL-05 AC2): a preview module lists with
// Connect disabled; with a Team allowlist fixture, a plugin off the list
// can't be connected and says why, while one on it still can.
describe("E2E-M12-6 preview and allowlist", () => {
  before(async function () {
    this.timeout(60_000);
    await freshOffline();
    await openPlugins();
  });

  after(async () => {
    await invoke("test_plugin_allowlist", { pluginIds: null });
  });

  it("F-PL-09.2 a preview module shows Preview and Connect is disabled", async () => {
    const figma = $('[data-testid="plugin-card-figma"]');
    await figma.waitForDisplayed({ timeout: 10_000 });
    expect(await figma.getProperty("textContent")).toContain("Preview");
    expect(await $('[data-testid="plugin-connect-figma"]').isEnabled()).toBe(false);
    expect(await $('[data-testid="plugin-connect-github"]').isEnabled()).toBe(true);
  });

  it("F-PL-05.2 a plugin off the Team allowlist can't be connected, with the reason", async function () {
    this.timeout(30_000);
    await invoke("test_plugin_allowlist", { pluginIds: ["github"] });
    const linear = $('[data-testid="plugin-card-linear"]');
    await browser.waitUntil(async () => (await linear.getAttribute("data-state")) === "blocked_by_org", {
      timeout: 10_000,
      timeoutMsg: "Linear never showed as blocked",
    });
    expect(await $('[data-testid="plugin-connect-linear"]').isEnabled()).toBe(false);
    expect(await $('[data-testid="plugin-why-linear"]').getText()).toBe(
      "Your organization doesn't allow Linear.",
    );
    expect(await $('[data-testid="plugin-connect-github"]').isEnabled()).toBe(true);
    // The backend refuses it too, not just the button.
    await expect(invoke("plugin_connect_oauth", { pluginId: "linear", connectionId: null })).rejects.toThrow(
      /forbidden_plan/,
    );
  });

  it("UI-90 the plugins page has no serious accessibility problems", async function () {
    this.timeout(60_000);
    await expectNoSeriousA11yViolationsInBothThemes();
  });
});
