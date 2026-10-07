import { $, browser } from "@wdio/globals";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { addFolder, checkAgain, fakeEngine, freshOffline, home, paneIds, setRegistryOverrides, waitForPaneText } from "../support/app";

// E2E-M4-7 Remote registry (F-EN-02 AC1): an engine added by the signed
// remote config is detected and launches in a terminal, no code change.
describe("E2E-M4-7 remote registry", () => {
  after(async () => {
    await setRegistryOverrides([]);
    rmSync(join(home(), "fakebin", "zed-agent"), { force: true });
    await checkAgain().catch(() => undefined);
  });

  it("F-EN-02.1 a config entry adds zed-agent: detected, offered and launched", async () => {
    fakeEngine("zed-agent", join(home(), "fakebin"));
    await setRegistryOverrides([{ id: "zed-agent", label: "Zed Agent", vendor: "Zed Industries", logo: "Z", commands: ["zed-agent"] }]);
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m4-7-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).toContain("zed-agent");
    const [first] = await paneIds();
    await $(`[data-testid="pane"][data-pane-id="${first}"] button[aria-label^="Close "]`).click();
    await $('[data-testid="tile-zed-agent"]').waitForClickable({ timeout: 10_000 });
    await $('[data-testid="tile-zed-agent"]').click();
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 15_000 });
    const [pane] = await paneIds();
    // The fake engine names itself by the command it runs as.
    await waitForPaneText(pane ?? "", "Fake zed-agent (test engine)");
  });
});
