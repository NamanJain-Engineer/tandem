import { existsSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { $, browser } from "@wdio/globals";
import { home, invoke, relaunch, resetToWelcome } from "../support/app";

// Early-access builds (D-105): no hosted service yet. The welcome screen
// says everything runs free with no account (D-869), the limits are the
// unlocked local set (D-2900), sign-in is refused, and the app sends
// nothing to the API. `--test-no-hosted` is the debug stand-in for a
// release built with TANDEM_HOSTED=0.
describe("EA-1 early-access build", () => {
  const log = () => join(home(), "net.log");
  const size = () => (existsSync(log()) ? statSync(log()).size : 0);

  // Back to a hosted build, signed out with no offline session, as a fresh
  // launch would be (E2E-M2-1 expects the sign-in screen).
  after(async () => {
    await relaunch();
    await invoke("auth_sign_out");
  });

  it("runs free and unlimited locally, with no sign-in and nothing sent to the API", async () => {
    const before = size();
    await relaunch(["--test-no-hosted"]);
    await resetToWelcome();
    await $('[data-testid="signin-free"]').waitForDisplayed({ timeout: 10_000 });
    expect(await $('[data-testid="signin-free"]').getText()).toBe(
      "Free · no account · nothing sent to Vibezeo",
    );
    expect(await $("button=Continue with Google").isExisting()).toBe(false);

    await $("button=Start using Vibezeo").click();
    await $('[data-testid="app-shell"]').waitForExist({ timeout: 20_000 });
    const { limits } = await invoke<{ limits: Record<string, unknown> }>("entitlement_get");
    expect(limits).toMatchObject({
      panes: null,
      workspaces: null,
      threadGrid: 6,
      chatEngines: null,
      routines: true,
      taskExecution: true,
      sessionSearch: true,
      conductor: false,
      cloudStt: false,
    });

    let refused = "";
    await invoke("auth_sign_in").catch((e: Error) => {
      refused = e.message;
    });
    expect(refused).toContain('"code":"unavailable"');

    // Startup work (remote config, entitlements, detection) has had time to run.
    await browser.pause(3_000);
    const sent = existsSync(log()) ? readFileSync(log(), "utf8").slice(before) : "";
    expect(sent).toBe("");
  });
});
