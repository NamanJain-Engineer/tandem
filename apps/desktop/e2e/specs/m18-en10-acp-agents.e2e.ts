import { $, browser } from "@wdio/globals";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { Key } from "webdriverio";
import { fakeEngine, freshUser, home, invoke, mod, planFooter, setPlan } from "../support/app";
import { composer, sendMessage, threadMode, waitForAssistant } from "../support/thread";

type Agent = { id: string; label: string; program: string; args: string[] };
type View = {
  spec: { id: string; label: string };
  install: { path: string } | null;
  chatCapable: boolean;
  added: boolean;
};
type Thread = { id: string };

/** The fake agent lives outside the login PATH: it's added by its full path. */
const agentDir = () => join(home(), "agents");

async function openEngines(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys([mod(), ","]);
  const nav = $('//nav[@aria-label="Settings pages"]//button[normalize-space()="Engines"]');
  await nav.waitForClickable({ timeout: 10_000 });
  await nav.click();
  await $('[data-testid="acp-agents"]').waitForDisplayed({ timeout: 10_000 });
}

async function closeSettings(): Promise<void> {
  await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
  await browser.keys(Key.Escape);
  await $('[data-testid="engines-page"]').waitForExist({ reverse: true, timeout: 5_000 });
}

/** Clicks by test id, found fresh (the section re-renders as it loads). */
async function press(testId: string): Promise<void> {
  await browser.waitUntil(
    () =>
      browser.execute((id: string) => {
        const el = document.querySelector<HTMLButtonElement>(`[data-testid="${id}"]`);
        if (!el || el.disabled) return false;
        el.click();
        return true;
      }, testId),
    { timeout: 10_000, timeoutMsg: `${testId} wasn't clickable` },
  );
}

async function fill(label: string, command: string, args: string): Promise<void> {
  await $('[data-testid="acp-label"]').setValue(label);
  await $('[data-testid="acp-command"]').setValue(command);
  await $('[data-testid="acp-args"]').setValue(args);
}

const agents = () => invoke<Agent[]>("acp_agents_list");

// E2E-M18-EN10 ACP agents (F-EN-10): a command that never answers isn't
// added; the fake agent added by its full path passes the test, is listed
// "via ACP" in the Thread engine picker, shows Pro's limit on Free and runs
// a turn on Pro; a secret-shaped variable is refused before anything runs.
describe("E2E-M18-EN10 ACP agents", function () {
  this.timeout(240_000);
  let email = "";
  let program = "";

  before(async () => {
    ({ email } = await freshUser("en10"));
    program = fakeEngine("my-agent", agentDir());
  });

  it("F-EN-10.2 a command that never answers initialize isn't added", async () => {
    await openEngines();
    await press("acp-add-open");
    await $('[data-testid="acp-add"]').waitForDisplayed({ timeout: 5_000 });
    await fill("Sleepy", "/bin/sleep", "30");
    const started = Date.now();
    await press("acp-add-confirm");
    const error = $('[data-testid="acp-error"]');
    await error.waitForDisplayed({ timeout: 20_000 });
    expect(await error.getText()).toBe("Not an ACP agent: it didn't answer within 10 seconds");
    expect(Date.now() - started).toBeGreaterThanOrEqual(9_000);
    expect(await agents()).toEqual([]);
    expect(existsSync(join(home(), "acp-agents.json"))).toBe(false);
  });

  it("F-EN-10.4 a variable that looks like a key is refused before anything runs", async () => {
    await expect(
      invoke("acp_agent_test", {
        input: {
          label: "Keyed",
          command: program,
          args: ["acp"],
          env: { OPENAI_API_KEY: "sk-proj-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789" },
        },
      }),
    ).rejects.toThrow(/Keep keys in the agent's own sign-in/);
  });

  it("F-EN-10.1 the fake agent added by its full path passes the test and is a Thread engine via ACP", async () => {
    await browser.execute(() => {
      const input = document.querySelector<HTMLInputElement>('[data-testid="acp-label"]');
      if (input) input.select();
    });
    await $('[data-testid="acp-label"]').clearValue();
    await $('[data-testid="acp-command"]').clearValue();
    await $('[data-testid="acp-args"]').clearValue();
    await fill("My agent", program, "acp");
    await press("acp-test");
    const tested = $('[data-testid="acp-tested"]');
    await tested.waitForDisplayed({ timeout: 15_000 });
    expect(await tested.getText()).toBe("my-agent answered, version 1.0.0-fake. It signs in with: Log in.");
    await press("acp-add-confirm");
    await $('[data-testid="acp-add"]').waitForExist({ reverse: true, timeout: 15_000 });
    const saved = await agents();
    expect(saved).toEqual([
      expect.objectContaining({ id: "acp-my-agent", label: "My agent", program, args: ["acp"] }),
    ]);
    await $('[data-testid="acp-agent"][data-id="acp-my-agent"]').waitForDisplayed({ timeout: 10_000 });
    const views = await invoke<View[]>("engines_list", { hostId: null });
    const view = views.find((v) => v.spec.id === "acp-my-agent");
    expect(view).toMatchObject({ added: true, chatCapable: true, install: { path: program } });
    await closeSettings();
  });

  it("F-EN-10.3 on Free the added agent shows the plan's limit in the picker", async () => {
    await threadMode();
    const thread = await invoke<Thread>("thread_create", {
      args: { kind: "thread", workspaceId: null, engineId: "claude" },
    });
    const row = $(`[data-testid="thread-row"][data-thread-id="${thread.id}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await composer().waitForDisplayed({ timeout: 10_000 });
    await $('[data-testid="composer"] [aria-label="Engine"]').click();
    const option = $('//*[@role="option"][contains(normalize-space(), "My agent · via ACP")]');
    await option.waitForDisplayed({ timeout: 10_000 });
    expect(await option.getAttribute("aria-disabled")).toBe("true");
    expect(await option.getText()).toContain("Pro");
    await browser.keys(Key.Escape);
  });

  it("F-EN-10.1 on Pro a thread with the added agent runs a turn", async () => {
    await setPlan(email, "pro");
    await invoke("entitlement_refresh");
    await browser.waitUntil(async () => (await planFooter()).startsWith("PRO"), { timeout: 20_000 });
    const thread = await invoke<Thread>("thread_create", {
      args: { kind: "thread", workspaceId: null, engineId: "acp-my-agent" },
    });
    const row = $(`[data-testid="thread-row"][data-thread-id="${thread.id}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await composer().waitForDisplayed({ timeout: 10_000 });
    await sendMessage("hello");
    await waitForAssistant("You said: hello");
  });
});
