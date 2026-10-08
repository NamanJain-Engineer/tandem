import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { $, browser } from "@wdio/globals";
import { freshUser, home, mod } from "../support/app";

// E2E-M2-7 Nothing leaves by default (MILESTONES M2, F-TL-01 AC1).
describe("E2E-M2-7 nothing leaves by default", () => {
  it("a session with telemetry off sends nothing to analytics or crash hosts", async () => {
    await freshUser("telemetry");
    const net = readFileSync(join(home(), "net.log"), "utf8");
    expect(net).toContain("/v1/me");
    expect(net).not.toMatch(/posthog|sentry/);
  });

  it("turning stats on then off deletes the analytics id", async () => {
    const idFile = join(home(), "telemetry", "analytics-id");
    await browser.keys([mod(), ","]);
    await $("button=Privacy & Telemetry").click();
    const toggle = $('[data-setting="telemetry_usage"] button[role="switch"]');
    await toggle.click();
    await browser.waitUntil(() => existsSync(idFile), {
      timeout: 5_000,
      timeoutMsg: "no analytics id after opting in",
    });
    await toggle.click();
    await browser.waitUntil(() => !existsSync(idFile), {
      timeout: 5_000,
      timeoutMsg: "analytics id kept after opting out",
    });
    await browser.keys("Escape");
  });
});
