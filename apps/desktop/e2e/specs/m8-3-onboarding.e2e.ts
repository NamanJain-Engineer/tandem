import { $, browser } from "@wdio/globals";
import { freshOffline, invoke, mod } from "../support/app";
import { key, setSetting } from "../support/dictate";

// E2E-M8-6 Dictate's setup (F-DI-11): Try it passes only once both the press
// and the release arrived. First run no longer has a Dictate step (F-SH-08,
// D-2647): the setup lives in Settings › Dictate, and first run points there
// with a tip after the first agent finishes. The wizard is E2E-M5-9's.

describe("E2E-M8-6 Dictate's setup", function () {
  this.timeout(300_000);

  it("E2E-M8-6 F-DI-11.2 Settings › Dictate › Try it passes only after press and release", async function () {
    await freshOffline();
    await setSetting("dictate_enabled", true);
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await browser.keys([mod(), ","]);
    const nav = $('//nav[@aria-label="Settings pages"]//button[normalize-space()="Dictate"]');
    await nav.waitForClickable({ timeout: 10_000 });
    await nav.click();
    await $('[data-testid="dictate-try"]').waitForClickable({ timeout: 15_000 });
    await $('[data-testid="dictate-try"]').click();
    await $('[data-testid="dictate-setup"]').waitForDisplayed({ timeout: 10_000 });

    await $('[data-testid="dictate-verify"]').click();
    const tryIt = $('[data-testid="dictate-try-it"]');
    await key("hold", "down");
    await browser.waitUntil(async () => (await tryIt.getAttribute("data-phase")) === "down", {
      timeout: 5_000,
      timeoutMsg: "the press wasn't seen",
    });
    expect(await $('[data-testid="dictate-key-detected"]').isExisting()).toBe(false);
    await browser.pause(300);
    await key("hold", "up");
    await $('[data-testid="dictate-key-detected"]').waitForDisplayed({ timeout: 5_000 });
    expect((await invoke<{ verified: boolean }>("dictate_status")).verified).toBe(true);
    await browser.keys("Escape");
  });
});
