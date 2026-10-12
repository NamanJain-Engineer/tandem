import { $, browser } from "@wdio/globals";
import { Key } from "webdriverio";
import { freshOffline, invoke, mod } from "../support/app";

// F-CN-06 (UI-67): Settings › Conductor. Memory is off by default and
// nothing can be written then; turned on, memory.md is viewable, editable
// and clearable from Settings, and a credential is refused.

type Memory = { enabled: boolean; text: string; facts: string[]; proposals: unknown[]; path: string };

// A fake key, assembled so secret scanners don't flag the spec.
const FAKE_KEY = ["sk", "proj", "Qdxidq7OyFjVisrM1h8vbvTXfQNNTGnNZlhyg74g"].join("-");

async function openConductor(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys([mod(), ","]);
  const nav = $('//nav[@aria-label="Settings pages"]//button[normalize-space()="Conductor"]');
  await nav.waitForClickable({ timeout: 10_000 });
  await nav.click();
  await $('[data-testid="settings-page-conductor"]').waitForDisplayed({ timeout: 10_000 });
}

describe("F-CN-06 Conductor persona and memory", () => {
  before(async function () {
    this.timeout(60_000);
    await freshOffline();
    await invoke("settings_set", { key: "conductor_memory", value: false });
    await invoke("conductor_memory_clear");
  });

  after(async () => {
    await invoke("conductor_memory_clear");
    await invoke("settings_set", { key: "conductor_memory", value: false });
  });

  it("F-CN-06.1 memory is off by default and nothing is written", async () => {
    await openConductor();
    await $("h2=Memory is off").waitForDisplayed({ timeout: 10_000 });
    await expect(invoke("conductor_memory_save", { text: "- Likes tea\n" })).rejects.toThrow(/unavailable/);
    const m = await invoke<Memory>("conductor_memory_get");
    expect(m.enabled).toBe(false);
    expect(m.text).toBe("");
    expect(m.path).toMatch(/conductor[/\\]memory\.md$/);
  });

  it("F-CN-06 the persona rows are on the page", async () => {
    expect(await $('[data-setting="conductor_name"]').isDisplayed()).toBe(true);
    expect(await $('[data-setting="conductor_tone"]').isDisplayed()).toBe(true);
    expect(await $('[data-setting="conductor_wake"]').isExisting()).toBe(true);
    expect(await $('[data-testid="conductor-about"]').isExisting()).toBe(true);
  });

  it("F-CN-06 turned on, memory.md is editable, refuses a credential and clears", async () => {
    await $('button[role="switch"][aria-label="Remember things between calls"]').click();
    const editor = $('//label[normalize-space()="memory.md"]/following::textarea[1]');
    await editor.waitForEnabled({ timeout: 10_000 });
    await editor.setValue("- Works on the billing service\n");
    await $("button=Save").click();
    await browser.waitUntil(async () => (await invoke<Memory>("conductor_memory_get")).facts.includes("Works on the billing service"), {
      timeout: 10_000,
      timeoutMsg: "the fact wasn't saved",
    });

    await editor.setValue(`- my token is ${FAKE_KEY}\n`);
    await $("button=Save").click();
    await $(`//*[contains(text(), "That looks like a secret, so Conductor won't remember it.")]`).waitForDisplayed({ timeout: 10_000 });
    expect((await invoke<Memory>("conductor_memory_get")).text).not.toContain(FAKE_KEY);

    await $("button=Forget everything…").click();
    const confirm = $('[role="alertdialog"]').$('button=Forget everything');
    await confirm.waitForClickable({ timeout: 5_000 });
    await confirm.click();
    await browser.waitUntil(async () => (await invoke<Memory>("conductor_memory_get")).text === "", {
      timeout: 10_000,
      timeoutMsg: "memory wasn't cleared",
    });
    await browser.keys(Key.Escape);
  });
});
