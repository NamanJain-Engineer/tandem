import { $, browser } from "@wdio/globals";
import { addFolder, fakeLog, freshUser, invoke, mod, paneIds } from "../support/app";
import { auditEvents, testRoute, waitForEntitlement } from "../support/hosted";
import { composer, sendMessage, threadMode, waitForAssistant } from "../support/thread";

type Thread = { id: string; access: string };

/** A thread in the first workspace, opened in Thread mode. */
async function openThread(): Promise<string> {
  await threadMode();
  const [ws] = await invoke<Array<{ id: string }>>("workspace_list");
  const { id } = await invoke<Thread>("thread_create", { args: { kind: "thread", workspaceId: ws?.id } });
  const row = await $(`[data-testid="thread-row"][data-thread-id="${id}"]`);
  await row.waitForExist({ timeout: 10_000 });
  await row.click();
  await composer().waitForDisplayed({ timeout: 10_000 });
  return id;
}

/** The access picker's options and whether each is disabled. */
async function accessOptions(): Promise<Array<{ text: string; disabled: boolean }>> {
  await $('[data-testid="composer"] [role="combobox"][aria-label="Access"]').click();
  await $('[role="option"]').waitForExist({ timeout: 5_000 });
  const options = await browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[role="option"]')).map((o) => ({
      text: o.textContent?.trim() ?? "",
      disabled: o.hasAttribute("data-disabled") || o.getAttribute("aria-disabled") === "true",
    })),
  );
  await browser.keys("Escape");
  return options;
}

/** The `--permission-mode` of the newest claude session the fake engine saw. */
function lastPermissionMode(): string {
  const start = fakeLog()
    .filter((l) => l.startsWith("start: claude"))
    .at(-1);
  return start?.match(/"--permission-mode", "([^"]+)"/)?.[1] ?? "";
}

// E2E-M13-4 Team (the desktop half: the web half is apps/web/e2e/m13-hosted)
// and E2E-M13-5 Audit (MILESTONES M13; F-BL-06 AC1, F-TL-10 AC1).
describe("E2E-M13-4 Team policy on the desktop", () => {
  const root = `/tmp/tandem-e2e-m13-team-${Date.now()}`;
  let email = "";

  before(async function () {
    this.timeout(120_000);
    ({ email } = await freshUser("team"));
    await testRoute("team", {
      owner: "owner@tandem.test",
      members: [email],
      policy: { maxAccess: "edits", auditEnabled: true, forceTelemetryOff: true },
    });
    await waitForEntitlement((e) => e.plan === "team" && e.policy?.maxAccess === "edits", "Team with max access Edits");
    await addFolder(root);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
  });

  it("F-BL-06.1 Full is disabled in the composer, with the reason", async function () {
    this.timeout(60_000);
    await openThread();
    const options = await accessOptions();
    const full = options.find((o) => o.text.startsWith("Full access"));
    expect(full?.disabled).toBe(true);
    // A disabled option says why.
    expect(full?.text).toContain("Set by your organization");
    expect(options.find((o) => o.text.startsWith("Edits"))?.disabled).toBe(false);
    expect(await $('[role="combobox"][aria-label="Access"]').isExisting()).toBe(true);
  });

  it("F-BL-06.1 a thread that asks for Full anyway runs with Edits", async function () {
    this.timeout(60_000);
    const id = await openThread();
    // What a forged client request would do: set Full directly in the core.
    await invoke("thread_update", { threadId: id, access: "full" });
    await sendMessage("hello team");
    await waitForAssistant("You said: hello team");
    expect(lastPermissionMode()).toBe("acceptEdits");
  });

  it("Settings › Privacy shows the org's locks and the audit notice", async function () {
    this.timeout(30_000);
    await browser.keys([mod(), ","]);
    await $("button=Privacy & Telemetry").click();
    await browser.waitUntil(async () => $('[data-testid="locked-telemetry_usage"]').isExisting(), {
      timeout: 10_000,
      timeoutMsg: "the usage-statistics toggle isn't locked",
    });
    expect(await $('[data-testid="audit-notice"]').isExisting()).toBe(true);
    await browser.keys("Escape");
  });
});

describe("E2E-M13-5 audit", () => {
  const root = `/tmp/tandem-e2e-m13-audit-${Date.now()}`;

  it("F-TL-10.1 a Team member's Full session start is an audit row", async function () {
    this.timeout(120_000);
    const { email } = await freshUser("audit");
    await testRoute("team", {
      owner: "owner@tandem.test",
      members: [email],
      policy: { maxAccess: "full", auditEnabled: true },
    });
    await waitForEntitlement((e) => e.plan === "team" && e.policy?.auditEnabled === true, "Team with audit on");
    await addFolder(root);
    const id = await openThread();
    await invoke("thread_update", { threadId: id, access: "full" });
    await sendMessage("audit me");
    await waitForAssistant("You said: audit me");
    expect(lastPermissionMode()).toBe("bypassPermissions");
    await browser.waitUntil(
      async () => (await auditEvents(email)).some((e) => e.event === "full_access_used" && e.meta.engine === "claude"),
      { timeout: 10_000, timeoutMsg: `no audit row: ${JSON.stringify(await auditEvents(email))}` },
    );
  });

  it("F-TL-10.1 a Pro user's Full session sends nothing", async function () {
    this.timeout(120_000);
    const { email } = await freshUser("audit-pro");
    await testRoute("plan", { email, plan: "pro" });
    await waitForEntitlement((e) => e.plan === "pro", "Pro");
    await addFolder(`${root}-pro`);
    const id = await openThread();
    await invoke("thread_update", { threadId: id, access: "full" });
    await sendMessage("no audit");
    await waitForAssistant("You said: no audit");
    expect(lastPermissionMode()).toBe("bypassPermissions");
    await browser.pause(2_000);
    expect(await auditEvents(email)).toEqual([]);
  });
});
