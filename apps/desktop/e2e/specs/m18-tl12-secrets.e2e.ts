import { $, browser } from "@wdio/globals";
import { Key } from "webdriverio";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  closeAllPanes,
  freshOffline,
  invoke,
  mod,
  paneIds,
  paneText,
  sendToPane,
  waitForPaneText,
} from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";

// E2E-M18-TL12 Agent-proof secrets (F-TL-12), with the fake keychain: a
// secret added on Settings › Secrets shows only its reference; a folder's
// .env refers to it; Run with secrets gives the value to that one command,
// masks it in the pane and leaves the shell after it without it.

const VALUE = "sk_test_e2e_4242";
const ENV = "API_KEY=vibezeo-secret://stripe-test\nPORT=3000\n";

async function press(selector: string): Promise<void> {
  await $(selector).waitForExist({ timeout: 10_000 });
  await browser.execute((sel: string) => document.querySelector<HTMLElement>(sel)?.click(), selector);
}

async function openSecrets(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys([mod(), ","]);
  const nav = $('//nav[@aria-label="Settings pages"]//button[normalize-space()="Secrets"]');
  await nav.waitForClickable({ timeout: 10_000 });
  await nav.click();
  await $('[data-testid="settings-page-secrets"]').waitForDisplayed({ timeout: 10_000 });
}

async function closeSettings(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys(Key.Escape);
  await $('[data-testid="settings-page-secrets"]').waitForExist({ reverse: true, timeout: 5_000 });
}

describe("E2E-M18-TL12 agent-proof secrets", function () {
  this.timeout(180_000);
  const folder = `/tmp/tandem-e2e-m18-tl12-${Date.now()}`;
  let first = "";

  before(async function () {
    this.timeout(60_000);
    mkdirSync(folder, { recursive: true });
    writeFileSync(join(folder, ".env"), ENV);
    await freshOffline();
    await addFolder(folder);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    first = (await paneIds())[0] ?? "";
  });

  after(async () => {
    await closeAllPanes();
    for (const s of await invoke<Array<{ name: string }>>("secrets_list"))
      await invoke("secrets_delete", { name: s.name });
    rmSync(folder, { recursive: true, force: true });
  });

  it("F-TL-12.1 an added secret shows its reference, never its value", async () => {
    await openSecrets();
    await press('[data-testid="secret-add"]');
    await $('[data-testid="secret-name"]').setValue("stripe-test");
    await $('[data-testid="secret-value"]').setValue(VALUE);
    expect(await $('[data-testid="secret-value"]').getAttribute("type")).toBe("password");
    await press('[data-testid="secret-save"]');
    const row = '[data-testid="secret-row"][data-name="stripe-test"]';
    await $(row).waitForExist({ timeout: 10_000 });
    expect(await $(`${row} [data-testid="secret-reference"]`).getText()).toBe("vibezeo-secret://stripe-test");
    expect(await browser.execute(() => document.body.innerText)).not.toContain(VALUE);
    expect(await invoke<Array<{ name: string }>>("secrets_list")).toEqual([
      expect.objectContaining({ name: "stripe-test" }),
    ]);
    await expectNoSeriousA11yViolationsInBothThemes();
    await closeSettings();
  });

  it("F-TL-12.2 F-TL-12.3 F-TL-12.5 Run with secrets gives the command the value, masks it, and the shell after it doesn't have it", async () => {
    await press(`[data-testid="pane"][data-pane-id="${first}"] [data-testid="pane-run"]`);
    await $('[data-testid="run-dialog"]').waitForDisplayed({ timeout: 10_000 });
    await browser.waitUntil(
      async () =>
        (
          await browser.execute(() =>
            Array.from(document.querySelectorAll('[data-testid="run-var"]')).map((v) => v.textContent ?? ""),
          )
        ).join("|") === "API_KEY → stripe-test|PORT",
      { timeout: 10_000, timeoutMsg: "the plan never showed" },
    );
    await expectNoSeriousA11yViolationsInBothThemes();
    await $('[data-testid="run-command"]').setValue(`test "$API_KEY" = ${VALUE} && echo "ok $PORT $API_KEY"`);
    const before = await paneIds();
    await press('[data-testid="run-start"]');
    await browser.waitUntil(async () => (await paneIds()).length === before.length + 1, {
      timeout: 15_000,
      timeoutMsg: "no run pane",
    });
    const run = (await paneIds()).find((p) => !before.includes(p)) ?? "";
    await waitForPaneText(run, "ok 3000 [REDACTED]");
    expect(await paneText(run)).not.toContain(VALUE);
    expect(readFileSync(join(folder, ".env"), "utf8")).toBe(ENV);
    await browser.pause(1_000);
    await sendToPane(run, 'echo "after=${API_KEY:-none}"\r');
    await waitForPaneText(run, "after=none");
  });

  it("F-TL-12.4 a reference to a missing secret is named and Run stays disabled", async () => {
    writeFileSync(join(folder, ".env"), "API_KEY=vibezeo-secret://gone\n");
    await press(`[data-testid="pane"][data-pane-id="${first}"] [data-testid="pane-run"]`);
    const v = '[data-testid="run-var"][data-name="API_KEY"]';
    await $(v).waitForExist({ timeout: 10_000 });
    expect(await $(v).getAttribute("data-missing")).toBe("true");
    expect(await $(v).getText()).toContain("No secret named gone");
    await $('[data-testid="run-command"]').setValue("true");
    expect(await $('[data-testid="run-start"]').isEnabled()).toBe(false);
    await browser.keys(Key.Escape);
    await $('[data-testid="run-dialog"]').waitForExist({ reverse: true, timeout: 5_000 });
    writeFileSync(join(folder, ".env"), ENV);
  });

  it("F-TL-12.6 Delete removes the secret", async () => {
    await openSecrets();
    await press('[data-testid="secret-row"][data-name="stripe-test"] [data-testid="secret-delete"]');
    const confirm = $('//div[@role="alertdialog" or @role="dialog"]//button[normalize-space()="Delete"]');
    await confirm.waitForClickable({ timeout: 5_000 });
    await confirm.click();
    await $('[data-testid="secret-row"]').waitForExist({ reverse: true, timeout: 10_000 });
    expect(await invoke<unknown[]>("secrets_list")).toEqual([]);
    await closeSettings();
  });
});
