import { $, browser } from "@wdio/globals";
import { freshProfile, invoke, pickFolder, relaunch, workspaceRoots } from "../support/app";
import { key, setSetting } from "../support/dictate";

// E2E-M8-6 Onboarding (F-DI-11, F-SH-08): the Dictate step's Try it passes
// only once both the press and the release arrived; quitting on the step
// resumes there; finishing lands in Code mode with the chosen folder. The
// rest of the wizard (skip everything, step 3 resume) is E2E-M5-9's.

const step = () => $('[data-testid="onboarding"]').getAttribute("data-step");
async function atStep(name: string): Promise<void> {
  await browser.waitUntil(
    async () => (await $('[data-testid="onboarding"]').isExisting()) && (await step()) === name,
    {
      timeout: 15_000,
      timeoutMsg: `the wizard never reached ${name}`,
    },
  );
}

describe("E2E-M8-6 Onboarding", () => {
  after(async () => {
    await invoke("onboarding_set", { done: true }).catch(() => undefined);
  });

  it("E2E-M8-6 Try it passes only after press and release; the step survives a relaunch; the folder opens in Code mode", async function () {
    this.timeout(180_000);
    await freshProfile();
    await setSetting("dictate_enabled", true);
    await $("#signin-title").waitForDisplayed({ timeout: 20_000 });
    await $("button=Use Vibezeo offline").click();
    await atStep("engines");
    await $('[data-testid="onboarding-skip"]').click();
    await atStep("accounts");
    await $('[data-testid="onboarding-continue"]').click();
    await atStep("dictate");
    await $('[data-testid="dictate-setup"]').waitForDisplayed({ timeout: 10_000 });

    // Quit on the Dictate step and come back to it.
    await relaunch();
    await atStep("dictate");

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

    await $('[data-testid="onboarding-continue"]').click();
    await atStep("privacy");
    await $('[data-testid="onboarding-continue"]').click();
    await atStep("workspace");
    const folder = pickFolder(`/tmp/tandem-e2e-m8-6-${Date.now()}`);
    await $('[data-testid="onboarding-choose-folder"]').click();
    await $('[data-testid="launch-grid"]').waitForDisplayed({ timeout: 20_000 });
    expect(await workspaceRoots()).toEqual([folder]);
  });
});
