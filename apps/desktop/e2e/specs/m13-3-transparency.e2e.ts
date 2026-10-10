import { readFileSync } from "node:fs";
import { join } from "node:path";
import { $, browser } from "@wdio/globals";
import { freshUser, home, invoke, mod } from "../support/app";

const row = (id: string) => $(`[data-testid="privacy-flows"] tr[data-flow="${id}"]`);

// E2E-M13-6 Transparency (MILESTONES M13, F-TL-07 AC1) and threat-model
// rule 7 on the real app: a signed-in session with every opt-in off sends
// nothing to the hosted features.
describe("E2E-M13-6 what leaves this machine", () => {
  const log = () => {
    try {
      return readFileSync(join(home(), "net.log"), "utf8");
    } catch {
      return "";
    }
  };
  // This spec's own lines: earlier specs in a run share the log.
  let from = 0;

  before(async function () {
    this.timeout(120_000);
    from = log().length;
    await freshUser("transparency");
  });

  it("a session with every opt-in off sends nothing hosted", () => {
    const net = log().slice(from);
    expect(net).toContain("/v1/me");
    expect(net).not.toMatch(/\/v1\/(ai|stt|audit)\/|posthog|sentry/);
  });

  it("F-TL-07.1 turning on cloud dictation turns its row On, with the destination", async function () {
    this.timeout(30_000);
    await browser.keys([mod(), ","]);
    await $("button=Privacy & Telemetry").click();
    await row("cloud_stt").waitForExist({ timeout: 10_000 });
    expect(await row("cloud_stt").getAttribute("data-status")).toBe("off");
    // The Dictate page arrives with M8; the setting it writes is this one.
    await invoke("settings_set", { key: "dictate_engine", value: "cloud" });
    await browser.waitUntil(async () => (await row("cloud_stt").getAttribute("data-status")) === "on", {
      timeout: 5_000,
      timeoutMsg: "the cloud dictation row didn't turn On",
    });
    const text = String(await row("cloud_stt").getProperty("textContent"));
    expect(text).toContain("On");
    expect(text).toContain("Audio of each take");
    expect(text).toContain("Vibezeo API → speech-to-text provider");
    await invoke("settings_set", { key: "dictate_engine", value: "local" });
    await browser.keys("Escape");
  });
});
