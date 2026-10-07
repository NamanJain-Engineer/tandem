import { $, browser } from "@wdio/globals";
import { existsSync, rmSync } from "node:fs";
import { join } from "node:path";
import {
  API,
  addFolder,
  freshUser,
  home,
  invoke,
  paneIds,
  planFooter,
  sendToPane,
  setPlan,
  split,
  textOf,
  waitForPaneText,
} from "../support/app";

const FEED = "http://127.0.0.1:8790";
type State = { stage: string; version: string | null; errorStage: string | null; upToDate: boolean; minVersion: { modal: boolean } | null };

async function scenario(name: "valid" | "badsig" | "lower" | "none"): Promise<void> {
  const res = await fetch(`${FEED}/__scenario`, { method: "POST", body: JSON.stringify({ scenario: name }) });
  if (!res.ok) throw new Error(`feed scenario ${name}: ${res.status}`);
}
async function minAppVersion(version?: string): Promise<string> {
  const res = await fetch(`${API}/__test/config`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(version ? { min_app_version: version } : {}),
  });
  return ((await res.json()) as { minAppVersion: string }).minAppVersion;
}
const installedFile = () => join(home(), "test-update-installed");
const installed = () => invoke<string | null>("test_update_installed");
const HOUR = 3_600_000;

// E2E-M5-7 Updater (F-SH-06) against scripts/mock-update-feed.ts.
describe("E2E-M5-7 updater", () => {
  let email = "";
  let agent = "";
  let originalMin = "0.0.0";

  before(async function () {
    this.timeout(120_000);
    ({ email } = await freshUser("m5-7"));
    originalMin = await minAppVersion();
    if (existsSync(installedFile())) rmSync(installedFile());
    await addFolder(`/tmp/tandem-e2e-m5-7-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
    // No scheduled check in the middle of a scenario.
    await invoke("settings_set", { key: "auto_check_updates", value: false });
  });

  after(async () => {
    await invoke("settings_set", { key: "auto_check_updates", value: true }).catch(() => undefined);
    await scenario("none").catch(() => undefined);
    await minAppVersion(originalMin).catch(() => undefined);
    await invoke("entitlement_refresh").catch(() => undefined);
  });

  it("F-SH-06.3 a lower version on the feed is never offered", async () => {
    await scenario("lower");
    const s = await invoke<State>("updates_check_now");
    expect(s.stage).toBe("idle");
    expect(s.upToDate).toBe(true);
    expect(await $('[data-testid="update-pill"]').isExisting()).toBe(false);
  });

  it("F-SH-06.2 a bad signature isn't installed and fails at the verify stage", async () => {
    await scenario("badsig");
    const s = await invoke<State>("updates_check_now");
    expect(s.stage).toBe("error");
    expect(s.errorStage).toBe("verify");
    const pill = await $('[data-testid="update-pill"]');
    await pill.waitForExist({ timeout: 5_000 });
    expect(await pill.getAttribute("data-stage")).toBe("verify");
    expect(await textOf('[data-testid="update-pill"]')).toBe("Update failed — Retry");
    expect(await installed()).toBeNull();
  });

  it("F-SH-06.1 a valid feed offers Restart to update; the dialog lists the live agent; the install succeeds", async () => {
    await scenario("valid");
    const s = await invoke<State>("updates_check_now");
    expect(s.stage).toBe("ready");
    expect(s.version).toBe("9.9.0");
    await browser.waitUntil(async () => (await textOf('[data-testid="update-pill"]')) === "Restart to update", { timeout: 5_000 });
    await $('[data-testid="update-pill"]').click();
    await $('[data-testid="update-restart-dialog"]').waitForExist({ timeout: 5_000 });
    expect(await textOf("[role=dialog] h2")).toBe("Restart to update to 9.9.0?");
    await browser.waitUntil(async () => (await $('[data-testid="update-live-agents"]').isExisting()), { timeout: 5_000 });
    const name = await textOf(`[data-testid="pane"][data-pane-id="${agent}"] [data-testid="pane-name"]`);
    expect(await textOf('[data-testid="update-live-agents"]')).toContain(name);
    await $('[data-testid="update-restart-now"]').click();
    await browser.waitUntil(async () => (await installed()) === "9.9.0", { timeout: 10_000, timeoutMsg: "the update wasn't installed" });
  });

  it("F-SH-06.5 with a lapsed plan the update still installs", async () => {
    if (existsSync(installedFile())) rmSync(installedFile());
    await setPlan(email, "free");
    await invoke("entitlement_refresh");
    await browser.waitUntil(async () => (await planFooter()).startsWith("FREE"), { timeout: 20_000 });
    const s = await invoke<State>("updates_check_now");
    expect(s.stage).toBe("ready");
    await invoke("updates_install");
    expect(await installed()).toBe("9.9.0");
  });

  it("F-SH-06.4 x-tandem-min-version: a banner while terminals keep working through 71 h, then a modal overlay at 73 h", async () => {
    await minAppVersion("99.0.0");
    await invoke("entitlement_refresh");
    await $('[data-testid="banner-unsupported"]').waitForExist({ timeout: 10_000, timeoutMsg: "no unsupported-version banner" });
    let s = await invoke<State>("test_min_version_shift", { ms: 71 * HOUR });
    expect(s.minVersion?.modal).toBe(false);
    await invoke("updates_state");
    expect(await $('[data-testid="unsupported-overlay"]').isExisting()).toBe(false);
    // The terminal still takes input at +71 h.
    await sendToPane(agent, "still here\r");
    await waitForPaneText(agent, "you said: still here");
    s = await invoke<State>("test_min_version_shift", { ms: 2 * HOUR });
    expect(s.minVersion?.modal).toBe(true);
    // The overlay comes back on focus (and every minute).
    await browser.execute(() => window.dispatchEvent(new Event("focus")));
    await $('[data-testid="unsupported-overlay"]').waitForExist({ timeout: 10_000 });
    expect(await textOf("#unsupported-title")).toBe("This version of Tandem is no longer supported");
    // Back to a supported minimum: the notice clears.
    await minAppVersion(originalMin);
    await invoke("entitlement_refresh");
    await browser.waitUntil(async () => (await invoke<State>("updates_state")).minVersion === null, { timeout: 10_000 });
    await browser.execute(() => window.dispatchEvent(new Event("focus")));
    await $('[data-testid="unsupported-overlay"]').waitForExist({ reverse: true, timeout: 10_000 });
  });
});
