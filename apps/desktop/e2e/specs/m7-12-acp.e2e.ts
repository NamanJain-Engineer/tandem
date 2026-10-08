import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { checkAgain, fakeEngine, freshOffline, home, invoke } from "../support/app";
import { assistantTexts, composer, sendMessage, threadMode, waitForAssistant } from "../support/thread";

type Thread = { id: string; model: string | null };
type Model = { id: string };

const models = (text: string) => writeFileSync(join(home(), "fake-models.txt"), `${text}\n`);
const pinned = '[data-testid="thread-pending"]';

/** Ends the thread's ACP agent, as a crash or an update would: the next
 * send starts a new session (which reports its models again). */
function killAgent(): void {
  try {
    execFileSync("pkill", ["-f", "opencode acp"]);
  } catch {
    // none running
  }
}

async function transcriptText(): Promise<string> {
  return browser.execute(
    () => document.querySelector<HTMLElement>('[data-testid="thread-main"] [role="log"]')?.innerText ?? "",
  );
}

// E2E-M7-12 ACP (F-TH-06): an Agent Client Protocol agent (the fake
// `opencode acp`) runs a thread: replies stream, a command asks inline, Stop
// cancels the prompt, the agent's models reach the picker and the chosen one
// is selected; when the agent stops offering it the thread falls back with
// "Model no longer offered by OpenCode"; a sign-in error offers Sign in.
describe("E2E-M7-12 ACP", () => {
  let threadId = "";

  before(async function () {
    this.timeout(60_000);
    models("opencode oc-fast,oc-deep");
    await freshOffline();
    fakeEngine("opencode", join(home(), "fakebin"));
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).toContain("opencode");
    await threadMode();
    threadId = (
      await invoke<Thread>("thread_create", {
        args: { kind: "thread", workspaceId: null, engineId: "opencode" },
      })
    ).id;
    const row = await $(`[data-testid="thread-row"][data-thread-id="${threadId}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await composer().waitForDisplayed({ timeout: 10_000 });
  });

  it("F-TH-06.1 a prompt streams, a command asks inline, and Stop cancels", async function () {
    this.timeout(90_000);
    await sendMessage("hello");
    await waitForAssistant("You said: hello");

    await sendMessage("run npm test");
    await $(pinned).waitForExist({ timeout: 20_000 });
    expect(await $(pinned).getText()).toContain("npm test");
    await $(`${pinned} [data-testid="approval-allow"]`).click();
    await waitForAssistant("Ran npm test.");

    await sendMessage("/hang");
    await browser.waitUntil(async () => (await assistantTexts()).some((t) => t.includes("Working on it")), {
      timeout: 20_000,
    });
    await composer().click();
    await browser.keys("Escape");
    await $("p*=Stopped by you").waitForExist({ timeout: 6_000 });
  });

  it("F-TH-06 the agent's models reach the picker, and the chosen one is used", async function () {
    this.timeout(60_000);
    const list = await invoke<Model[]>("models_list", { engineId: "opencode" });
    expect(list.map((m) => m.id)).toEqual(expect.arrayContaining(["oc-fast", "oc-deep"]));
    await invoke("thread_update", { threadId, model: "oc-deep" });
    await sendMessage("/model");
    await waitForAssistant("Model: oc-deep");
  });

  it("F-TH-06.2 a withdrawn model falls back with the note", async function () {
    this.timeout(60_000);
    models("opencode oc-fast");
    killAgent();
    await sendMessage("again");
    await waitForAssistant("You said: again");
    await browser.waitUntil(
      async () => (await transcriptText()).includes("Model no longer offered by OpenCode"),
      {
        timeout: 10_000,
        timeoutMsg: `transcript: ${await transcriptText()}`,
      },
    );
    const thread = await invoke<{ thread: Thread }>("thread_get", { threadId });
    expect(thread.thread.model).toBeNull();
    const list = await invoke<Model[]>("models_list", { engineId: "opencode" });
    expect(list.map((m) => m.id)).not.toContain("oc-deep");
  });

  it("F-TH-06 a sign-in error is its own stage and Sign in opens the engine's login", async function () {
    this.timeout(60_000);
    models("opencode signed-out");
    killAgent();
    await sendMessage("hi");
    const signIn = await $('[data-testid="thread-main"]').$("button=Sign in");
    await signIn.waitForExist({ timeout: 20_000 });
    expect(await transcriptText()).toContain("OpenCode needs you to sign in.");
    await signIn.click();
    await $('[data-testid="login-start"]').waitForExist({ timeout: 10_000 });
    await browser.keys("Escape");
    await $('[data-testid="login-start"]').waitForExist({ reverse: true, timeout: 5_000 });

    // Signed in again: the next turn starts a session that works.
    models("opencode oc-fast");
    await sendMessage("back");
    await waitForAssistant("You said: back");
  });
});
