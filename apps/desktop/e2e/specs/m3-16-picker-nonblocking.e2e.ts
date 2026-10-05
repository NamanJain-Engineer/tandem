import { $, browser } from "@wdio/globals";
import { mkdirSync, writeFileSync } from "node:fs";
import { freshOffline, home, mod, typeKeys, workspaceRoots } from "../support/app";

// E2E-M3-16 Picker non-blocking (F-CD-01 AC4): the fake picker holds for 4 s,
// as if the native dialog were open.
describe("E2E-M3-16 picker non-blocking", () => {
  it("F-CD-01.4 a palette command runs while the folder picker is open", async () => {
    await freshOffline();
    const folder = `/tmp/tandem-e2e-m3-16-${Date.now()}`;
    mkdirSync(folder, { recursive: true });
    writeFileSync(`${home()}/pick-folder.txt`, `delay=4000\n${folder}\n`);
    await $('nav[aria-label="Workspaces"] button[aria-label="Add folder…"]').click();
    const opened = Date.now();
    await browser.keys([mod(), "k"]);
    await $('input[placeholder="Type a command or search…"]').waitForDisplayed({ timeout: 3_000 });
    await typeKeys("Toggle left rail");
    await browser.keys("Enter");
    await browser.waitUntil(async () => (await $('nav[aria-label="Workspaces"]').getSize("width")) < 100, { timeout: 3_000, timeoutMsg: "the rail didn't collapse" });
    expect(Date.now() - opened).toBeLessThan(4_000);
    expect(await workspaceRoots()).not.toContain(folder);
    await browser.waitUntil(async () => (await workspaceRoots()).includes(folder), { timeout: 10_000 });
    await browser.keys([mod(), "\\"]);
  });
});
