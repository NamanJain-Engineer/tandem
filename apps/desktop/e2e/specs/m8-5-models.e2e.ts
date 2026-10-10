import { browser } from "@wdio/globals";
import { freshOffline, invoke } from "../support/app";
import {
  BASE_EN,
  ensureModel,
  fixture,
  key,
  mic,
  modelServer,
  models,
  resetPlatform,
  setSetting,
  waitPasted,
} from "../support/dictate";

// E2E-M8-8 Model download (F-DI-04): a corrupted byte fails the SHA-256
// check, deletes the file and offers Retry; a switch asked for mid-take
// applies once the take ends.

describe("E2E-M8-8 Model download", () => {
  before(async function () {
    this.timeout(300_000);
    await freshOffline();
    await setSetting("dictate_enabled", true);
  });
  after(() => modelServer("normal"));

  it("E2E-M8-8 a corrupt byte fails verification, deletes the file and offers Retry", async function () {
    this.timeout(180_000);
    let view = await models();
    if (view.models.find((m) => m.id === BASE_EN)?.state !== "absent")
      await invoke("dictate_model_remove", { modelId: BASE_EN });
    modelServer({ corruptByte: 1000 });
    await invoke("dictate_model_download", { modelId: BASE_EN });
    await browser.waitUntil(
      async () => {
        view = await models();
        return view.models.find((m) => m.id === BASE_EN)?.state === "failed";
      },
      { timeout: 120_000, interval: 300, timeoutMsg: "the corrupt download didn't fail" },
    );
    const row = view.models.find((m) => m.id === BASE_EN);
    expect(row?.error ?? "").toMatch(/didn't match/);
    // Nothing of it stays on disk: the manager reports no space used by it.
    expect(Number(view.usedBytes)).toBe(0);
    // Retry, with the bytes intact, succeeds.
    await ensureModel();
  });

  it("E2E-M8-8 switching model mid-take applies after the take ends", async function () {
    this.timeout(180_000);
    await ensureModel();
    const other = (await models()).models.find((m) => m.id !== BASE_EN && m.state === "downloaded");
    // The switch needs a second model on disk; Parakeet is in the cache when fetched.
    if (!other) {
      const candidate = (await models()).models.find(
        (m) => m.id.startsWith("parakeet") && m.available && m.state === "absent",
      );
      if (!candidate) throw new Error("no second model to switch to (run scripts/fetch-models.sh parakeet)");
      await invoke("dictate_model_download", { modelId: candidate.id });
      await browser.waitUntil(
        async () => (await models()).models.find((m) => m.id === candidate.id)?.state === "downloaded",
        {
          timeout: 300_000,
          interval: 500,
          timeoutMsg: `${candidate.id} didn't download`,
        },
      );
    }
    const target = (await models()).models.find((m) => m.id !== BASE_EN && m.state === "downloaded");
    if (!target) throw new Error("no second model");
    await resetPlatform();
    await mic({ source: "wav", path: fixture("hello_world_16k.wav") });
    await key("hold", "down");
    await browser.pause(500);
    await invoke("dictate_model_select", { modelId: target.id });
    const mid = await models();
    expect(mid.active).toBe(BASE_EN);
    expect(mid.pending).toBe(target.id);
    await browser.pause(2_000);
    await key("hold", "up");
    await waitPasted();
    await browser.waitUntil(async () => (await models()).active === target.id, {
      timeout: 60_000,
      timeoutMsg: "the switch never applied after the take",
    });
    await invoke("dictate_model_select", { modelId: BASE_EN });
  });
});
