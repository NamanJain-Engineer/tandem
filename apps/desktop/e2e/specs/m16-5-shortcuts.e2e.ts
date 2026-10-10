import { $, browser } from "@wdio/globals";
import { Key } from "webdriverio";
import { addFolder, freshOffline, invoke, mod, paneIds, relaunch, waitForPaneText } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";

// E2E-M16-5 Shortcuts (F-SH-09 P2, UI-113): in Settings › Shortcuts, a
// combo text editing owns is refused, and Mod+Alt+R recorded for "Split
// right" splits from then on while Mod+D no longer does, after a restart
// too. The cheatsheet (Mod+/) lists the new keys.

const row = (id: string) => $(`[data-testid="shortcut-${id}"]`);

async function until(check: () => Promise<boolean>, timeoutMsg: string, timeout = 10_000): Promise<void> {
  await browser.waitUntil(check, { timeout, timeoutMsg });
}

async function openShortcuts(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys([mod(), ","]);
  const nav = $('//nav[@aria-label="Settings pages"]//button[normalize-space()="Shortcuts"]');
  await nav.waitForClickable({ timeout: 10_000 });
  await nav.click();
  await $('[data-testid="settings-page-shortcuts"]').waitForDisplayed({ timeout: 10_000 });
}

async function keysOf(id: string): Promise<string> {
  return (await row(id).$('[data-testid="row-keys"]').getText()).replace(/\s+/g, "");
}

async function record(id: string, keys: string[]): Promise<void> {
  const button = row(id).$('button[aria-label^="Record a shortcut for"]');
  await button.click();
  await until(async () => (await button.getAttribute("data-recording")) === "true", "the recorder didn't start");
  await browser.keys(keys);
}

async function closeSettings(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys(Key.Escape);
  await $('[data-testid="settings-page-shortcuts"]').waitForExist({ reverse: true, timeout: 5_000 });
}

/** Presses keys with nothing focused and reports whether the pane picker (a split) opened. */
async function splits(keys: string[]): Promise<boolean> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys(keys);
  const picker = $('[data-testid="pane-picker"]');
  const opened = await picker.waitForDisplayed({ timeout: 1_500 }).then(
    () => true,
    () => false,
  );
  if (opened) {
    await browser.keys(Key.Escape);
    await picker.waitForDisplayed({ reverse: true, timeout: 5_000 });
  }
  return opened;
}

describe("E2E-M16-5 shortcuts", () => {
  before(async function () {
    this.timeout(60_000);
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m16-5-keys-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    await waitForPaneText((await paneIds())[0] ?? "", "#");
  });

  after(async () => {
    await invoke("settings_set", { key: "shortcut_overrides", value: {} });
  });

  it("F-SH-09.8 a combo text editing owns is refused and changes nothing", async () => {
    await openShortcuts();
    expect(await keysOf("pane.split_right")).toBe(process.platform === "darwin" ? "⌘D" : "CtrlD");
    await record("pane.split_right", [mod(), "v"]);
    await until(async () => (await row("pane.split_right").$('[role="alert"]').getText()) === "Your system uses this shortcut.", "no refusal");
    expect(await keysOf("pane.split_right")).toBe(process.platform === "darwin" ? "⌘D" : "CtrlD");
    expect(await invoke("settings_get_all").then((v) => (v as Record<string, unknown>).shortcut_overrides)).toEqual({});
    await expectNoSeriousA11yViolationsInBothThemes();
  });

  it("F-SH-09.6 Mod+Alt+R recorded for Split right splits, Mod+D doesn't, and both hold after a restart", async function () {
    this.timeout(120_000);
    await record("pane.split_right", [mod(), Key.Alt, "r"]);
    await until(async () => (await keysOf("pane.split_right")) === (process.platform === "darwin" ? "⌥⌘R" : "CtrlAltR"), "the new keys aren't shown");
    expect(await invoke("settings_get_all").then((v) => (v as Record<string, unknown>).shortcut_overrides)).toEqual({ "pane.split_right": "Mod+Alt+R" });
    await closeSettings();
    expect(await splits([mod(), Key.Alt, "r"])).toBe(true);
    expect(await splits([mod(), "d"])).toBe(false);

    await relaunch();
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    expect(await splits([mod(), Key.Alt, "r"])).toBe(true);
    expect(await splits([mod(), "d"])).toBe(false);
    // The cheatsheet lists the keys in effect.
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await browser.keys([mod(), "/"]);
    const entry = $('[data-testid="cheatsheet-row"][data-id="pane.split_right"]');
    await entry.waitForDisplayed({ timeout: 5_000 });
    expect((await entry.getText()).replace(/\s+/g, "")).toBe(process.platform === "darwin" ? "Splitright⌥⌘R" : "SplitrightCtrlAltR");
    await expectNoSeriousA11yViolationsInBothThemes();
    await browser.keys(Key.Escape);
  });
});
