import { browser } from "@wdio/globals";
import { freshUser, invoke } from "../support/app";
import {
  ensureModel,
  fixture,
  hold,
  mic,
  platform,
  resetPlatform,
  setSetting,
  waitPasted,
  words,
} from "../support/dictate";
import { testRoute, waitForEntitlement } from "../support/hosted";

// F-DI-10 Enhance and Polish against the dev API's mock rewrite (the text
// with its whitespace normalized): Auto-Enhance rewrites a take before it is
// inserted and settles 3 credits; a Polish quick action replaces the pasted
// text (undo, then paste) while its field still has focus.

describe("F-DI-10 Enhance and Polish", function () {
  this.timeout(300_000);
  let email = "";

  before(async () => {
    ({ email } = await freshUser("enhance"));
    await setSetting("dictate_enabled", true);
    await setSetting("dictate_engine", "local");
    await ensureModel();
    await testRoute("plan", { email, plan: "pro", paid: true });
    await testRoute("credits", { email, monthly: 1_000, purchased: 0 });
    await waitForEntitlement(
      (e) => e.plan === "pro" && e.credits?.monthly === 1_000,
      "Pro with 1,000 credits",
    );
    await setSetting("dictate_enhance_disclosed", true);
  });

  after(async () => {
    await setSetting("dictate_auto_enhance", false);
  });

  it("F-DI-10 Auto-Enhance rewrites the take before it is inserted and costs 3 credits", async () => {
    await setSetting("dictate_auto_enhance", true);
    await resetPlatform();
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    await hold(2_500);
    const view = await waitPasted();
    expect(words(view.pasted[0] ?? "")).toContain("hello world");
    await waitForEntitlement((e) => e.credits?.monthly === 997, "3 credits for the rewrite", 15_000);
    await setSetting("dictate_auto_enhance", false);
  });

  it("F-DI-10 a Polish quick action goes through the relay for 3 credits and leaves identical text alone", async () => {
    // The mock rewrite only normalizes whitespace, which the clean-up already
    // did: the answer equals the take, so nothing is undone or pasted again.
    // Replacing a different answer in place is the controller's tests' job.
    await resetPlatform();
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    await hold(2_500);
    await waitPasted();
    await browser.pause(800);
    expect(await invoke<boolean>("dictate_quick_action", { mode: "polish" })).toBe(true);
    await waitForEntitlement((e) => e.credits?.monthly === 994, "3 more credits", 15_000);
    await browser.pause(500);
    const view = await platform();
    expect(view.calls).not.toContain("undo");
    expect(view.pasted.length).toBe(1);
  });
});
