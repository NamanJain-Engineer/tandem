import { $, browser } from "@wdio/globals";
import type { AutoRunView } from "@tandem/protocol";
import {
  addFolder,
  fakeLog,
  freshUser,
  invoke,
  paneIds,
  paneStatus,
  sendToPane,
  split,
  waitForPaneText,
  waitForStatus,
} from "../support/app";
import { testRoute, waitForEntitlement } from "../support/hosted";

// The fake engine logs every read from its terminal as hex: the PTY write log.
const NEXT = Buffer.from("Continue with the next step.", "utf8").toString("hex");
const said = (from: number) =>
  fakeLog()
    .slice(from)
    .filter((l) => l.startsWith("raw: ") && l.includes(NEXT)).length;

async function runOn(pane: string): Promise<AutoRunView | undefined> {
  return (await invoke<AutoRunView[]>("auto_list")).find((r) => r.paneId === pane);
}

async function until(pane: string, ok: (r: AutoRunView) => boolean, what: string, timeout = 30_000): Promise<AutoRunView> {
  let last: AutoRunView | undefined;
  try {
    await browser.waitUntil(
      async () => {
        last = await runOn(pane);
        return last !== undefined && ok(last);
      },
      { timeout, interval: 200 },
    );
  } catch {
    // The run as last seen and the pane's status, not the state before polling.
    throw new Error(`${what}: ${JSON.stringify(last)}; pane status "${await paneStatus(pane)}"`);
  }
  return last as AutoRunView;
}

async function paneMenu(pane: string, item: string): Promise<void> {
  await $(`[data-testid="pane"][data-pane-id="${pane}"] button[aria-label^="Actions for"]`).click();
  await browser.waitUntil(
    async () =>
      browser.execute((label: string) => {
        const el = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(
          (x) => (x.textContent ?? "").trim() === label,
        );
        el?.click();
        return el !== undefined;
      }, item),
    { timeout: 5_000, timeoutMsg: `no menu item ${item}` },
  );
}

const chip = (pane: string) => $(`[data-testid="pane"][data-pane-id="${pane}"] [data-testid="auto-chip"]`);
const chipText = async (pane: string): Promise<string> =>
  (await chip(pane).isExisting()) ? String(await chip(pane).getProperty("textContent")) : "none";

// E2E-M16-7 Auto (F-AU-01): the mock supervisor answers "Continue with the
// next step." until the pane shows ALL TESTS PASSED; the fake engine answers
// every prompt and goes back to its prompt.
describe("E2E-M16-7 Auto", () => {
  let email = "";
  let agent = "";

  before(async function () {
    this.timeout(150_000);
    ({ email } = await freshUser("auto"));
    await testRoute("plan", { email, plan: "pro", paid: true });
    await testRoute("credits", { email, monthly: 10_000, purchased: 0 });
    await waitForEntitlement((e) => e.plan === "pro" && e.credits?.monthly === 10_000, "Pro with 10,000 credits");
    await addFolder(`/tmp/tandem-e2e-auto-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
  });

  async function freshAgent(): Promise<void> {
    const run = await runOn(agent);
    if (run) await invoke("auto_dismiss", { runId: run.runId });
  }

  it("F-AU-01 first use asks consent; a Custom run stops at exactly its ceiling", async function () {
    this.timeout(90_000);
    await freshAgent();
    const mark = fakeLog().length;
    await paneMenu(agent, "Auto…");
    await $('[data-testid="auto-dialog"]').waitForDisplayed({ timeout: 5_000 });
    await $('button[role="radio"][value="custom"]').click();
    await $('[data-testid="auto-goal"]').setValue("make the tests pass");
    const ceiling = $('[data-testid="auto-ceiling"]');
    await ceiling.clearValue();
    await ceiling.setValue("3");
    await $('[data-testid="auto-start"]').click();
    await $('[data-testid="auto-consent"]').waitForDisplayed({ timeout: 5_000 });
    expect(await $('[data-testid="auto-consent"]').getText()).toContain("secrets redacted");
    await $('[data-testid="auto-consent-continue"]').click();
    const done = await until(agent, (r) => r.state === "ended", "the run ends");
    expect(done.endReason).toBe("turn_ceiling");
    expect(done.turns).toBe(3);
    expect(said(mark)).toBe(3);
    await browser.waitUntil(async () => (await chipText(agent)).startsWith("Stopped"), { timeout: 5_000 });
    expect(await chipText(agent)).toContain("Reached the turn limit.");
    // 3 turns at 6 credits each.
    await waitForEntitlement((e) => e.credits?.monthly === 10_000 - 18, "10,000 − 18 credits", 15_000);
    const notes = await invoke<{ items: Array<{ title: string }> }>("notifications_list", {});
    expect(notes.items.some((n) => n.title === "Auto reached its turn limit")).toBe(true);
    await $(`[data-testid="pane"][data-pane-id="${agent}"] [data-testid="auto-dismiss"]`).click();
    await browser.waitUntil(async () => !(await chip(agent).isExisting()), { timeout: 5_000 });
  });

  it("F-AU-01.5 waiting for approval pauses Auto; it resumes once answered", async function () {
    this.timeout(60_000);
    await sendToPane(agent, "/ask\r");
    await waitForStatus(agent, "waiting");
    const mark = fakeLog().length;
    const view = await invoke<AutoRunView>("auto_start", { paneId: agent, mode: "custom", goal: "go on", ceiling: 1 });
    await until(agent, (r) => r.paused === "approval_pending", "paused for the approval");
    await browser.pause(1_500);
    expect(said(mark)).toBe(0);
    expect(await chipText(agent)).toContain("Paused");
    // The user answers the dialog; the agent finishes and is back at its prompt.
    await sendToPane(agent, "1\r");
    const end = await until(agent, (r) => r.state === "ended", "the run ends");
    expect(end.runId).toBe(view.runId);
    expect(end.endReason).toBe("turn_ceiling");
    expect(said(mark)).toBe(1);
    await invoke("auto_dismiss", { runId: end.runId });
  });

  it("F-AU-01.2 the engine exiting ends the run as engine exited, with no further writes", async function () {
    this.timeout(60_000);
    const view = await invoke<AutoRunView>("auto_start", { paneId: agent, mode: "custom", goal: "keep going", ceiling: 30 });
    await invoke("auto_pause", { runId: view.runId });
    await sendToPane(agent, "/exit\r");
    await waitForPaneText(agent, "Goodbye from the test engine");
    const end = await until(agent, (r) => r.state === "ended", "the run ends");
    expect(end.endReason).toBe("engine_exited");
    const mark = fakeLog().length;
    await browser.pause(1_500);
    expect(said(mark)).toBe(0);
    await invoke("auto_dismiss", { runId: end.runId });
  });

  it("F-AU-01.4 with 0 credits the first call is refused and the end reason is out of credits", async function () {
    this.timeout(90_000);
    agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
    // Auto only asks the supervisor once the agent is at its prompt; under a
    // loaded full run the new pane's first status can take a while.
    await waitForStatus(agent, "idle", 45_000);
    await testRoute("credits", { email, monthly: 0, purchased: 0 });
    await waitForEntitlement((e) => e.credits?.monthly === 0, "0 credits");
    const mark = fakeLog().length;
    await invoke("auto_start", { paneId: agent, mode: "review" });
    const end = await until(agent, (r) => r.state === "ended", "the run ends");
    expect(end.endReason).toBe("out_of_credits");
    expect(end.turns).toBe(0);
    expect(said(mark)).toBe(0);
  });
  it("F-AU-01 the supervisor's done ends the run with its reason and nothing typed", async function () {
    this.timeout(60_000);
    // On the second pane, with credits again.
    await testRoute("credits", { email, monthly: 10_000, purchased: 0 });
    await waitForEntitlement((e) => e.credits?.monthly === 10_000, "10,000 credits");
    const old = await runOn(agent);
    if (old) await invoke("auto_dismiss", { runId: old.runId });
    await sendToPane(agent, "ALL TESTS PASSED\r");
    await waitForPaneText(agent, "you said: ALL TESTS PASSED");
    await waitForStatus(agent, "done");
    const mark = fakeLog().length;
    await invoke("auto_start", { paneId: agent, mode: "finish" });
    const done = await until(agent, (r) => r.state === "ended", "the run ends");
    expect(done.endReason).toBe("done");
    expect(done.detail).toBe("The tests pass.");
    expect(said(mark)).toBe(0);
    await browser.waitUntil(async () => (await chipText(agent)).includes("Done · The tests pass."), {
      timeout: 5_000,
      timeoutMsg: `chip: ${await chipText(agent)}`,
    });
    await invoke("auto_dismiss", { runId: done.runId });
  });

});
