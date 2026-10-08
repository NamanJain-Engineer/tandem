import { $, browser } from "@wdio/globals";
import {
  addFolder,
  fakeLog,
  freshOffline,
  invoke,
  paneIds,
  sendToPane,
  split,
  waitForPaneText,
} from "../support/app";

type Account = { id: string; engineId: string; isDefault: boolean };

const text = async (selector: string) => String(await $(selector).getProperty("textContent"));

// E2E-M6-7 Limit (F-AC-05, UI-36): the fake engine's limit message shows the
// card; nothing switches account; Resume at reset types "continue" at the
// reset. The reset is seconds away on the real clock (the FakeClock case is
// unit-tested: `f_ac_05_2_resume_at_reset_…`).
describe("E2E-M6-7 limit", () => {
  it("F-AC-05.1 F-AC-05.2 a limit shows the card, no other account is used, and Resume at reset types continue", async function () {
    this.timeout(120_000);
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m6-7-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
    const accounts = await invoke<Account[]>("accounts_list");
    const panes = (await paneIds()).length;

    await sendToPane(agent, "/limit 8s\r");
    const banner = `[data-testid="pane"][data-pane-id="${agent}"] [data-testid="limit-banner"]`;
    await $(banner).waitForExist({ timeout: 15_000 });
    expect(await text(banner)).toMatch(/reached its limit · resets .+ \(in /);

    // Nothing moves on its own: same accounts, same panes.
    await browser.pause(1_500);
    expect(await invoke<Account[]>("accounts_list")).toEqual(accounts);
    expect((await paneIds()).length).toBe(panes);

    const mark = fakeLog().length;
    await $(`${banner} [data-testid="limit-resume-at-reset"]`).click();
    await browser.waitUntil(async () => (await $(banner).getAttribute("data-scheduled")) === "true", {
      timeout: 5_000,
    });
    expect(await text(banner)).toMatch(/^Paused · resumes /);
    expect(fakeLog().slice(mark)).not.toContain("line: continue");
    await browser.waitUntil(async () => fakeLog().slice(mark).includes("line: continue"), {
      timeout: 20_000,
      timeoutMsg: `fake log: ${JSON.stringify(fakeLog().slice(mark))}`,
    });
    await $(banner).waitForExist({ reverse: true, timeout: 5_000 });
  });
});
