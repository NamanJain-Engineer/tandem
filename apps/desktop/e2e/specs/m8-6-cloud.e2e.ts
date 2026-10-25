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

// F-DI-05 Cloud speech-to-text against the dev API's mock STT provider
// ("hello world"): a Pro take is transcribed in the cloud and settles
// credits; a 402 inserts nothing until "Use local for this take"; a Free
// plan sends no audio at all.

describe("F-DI-05 Cloud transcription", function () {
  this.timeout(300_000);
  let email = "";

  before(async () => {
    ({ email } = await freshUser("cloud-stt"));
    await setSetting("dictate_enabled", true);
    await setSetting("dictate_widget_visibility", "while_recording");
    await ensureModel();
    await testRoute("plan", { email, plan: "pro", paid: true });
    await testRoute("credits", { email, monthly: 1_000, purchased: 0 });
    await waitForEntitlement(
      (e) => e.plan === "pro" && e.credits?.monthly === 1_000,
      "Pro with 1,000 credits",
    );
    await setSetting("dictate_cloud_consented", true);
    await setSetting("dictate_engine", "cloud");
  });

  after(async () => {
    await setSetting("dictate_engine", "local");
  });

  it("F-DI-05.1 a cloud take inserts the relay's text and settles credits", async () => {
    await resetPlatform();
    await mic({ source: "wav", path: fixture("use_tandem_16k.wav") });
    await hold(2_500);
    const view = await waitPasted();
    // The mock provider answers "hello world" whatever was said: the words
    // came from the cloud, not the local model.
    expect(words(view.pasted[0] ?? "")).toBe("hello world");
    await waitForEntitlement(
      (e) => e.credits?.monthly === 998,
      "2 credits settled for one 15 s block",
      15_000,
    );
    const take = await invoke<{ takes: Array<{ engine: string }> }>("dictate_history", {
      query: null,
      before: null,
      limit: 1,
    });
    expect(take.takes[0]?.engine).toBe("cloud");
  });

  it("F-DI-05.2 a 402 inserts nothing; F-DI-05.3 local runs only when asked", async () => {
    await testRoute("credits", { email, monthly: 0, purchased: 0 });
    await waitForEntitlement((e) => e.credits?.monthly === 0, "no credits");
    await resetPlatform();
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    await hold(2_500);
    await browser.pause(4_000);
    expect((await platform()).pasted).toEqual([]);
    expect(await invoke<boolean>("dictate_use_local")).toBe(true);
    const view = await waitPasted();
    expect(words(view.pasted[0] ?? "")).toContain("hello world");
    expect(await invoke<boolean>("dictate_use_local")).toBe(false);
  });

  it("F-DI-05 an account without cloud STT sends no audio and offers local", async () => {
    await testRoute("plan", { email, plan: "free" });
    await waitForEntitlement((e) => e.plan === "free", "Free");
    await resetPlatform();
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    await hold(2_500);
    await browser.pause(2_000);
    expect((await platform()).pasted).toEqual([]);
    expect(await invoke<boolean>("dictate_use_local")).toBe(true);
    await waitPasted();
  });
});
