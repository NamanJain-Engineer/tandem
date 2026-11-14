import { $, browser } from "@wdio/globals";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { basename } from "node:path";
import {
  freshProfile,
  invoke,
  mod,
  paneIds,
  pickFolder,
  relaunch,
  sendToPane,
  textOf,
  typeKeys,
  waitForPaneText,
  waitForStatus,
  workspaceRoots,
} from "../support/app";

const step = () => $('[data-testid="onboarding"]').getAttribute("data-step");
async function atStep(name: string): Promise<void> {
  await browser.waitUntil(
    async () => (await $('[data-testid="onboarding"]').isExisting()) && (await step()) === name,
    {
      timeout: 15_000,
      timeoutMsg: `the wizard never reached ${name}`,
    },
  );
}
/** The toasts' titles, in the order they show. */
const toasts = async (): Promise<string[]> =>
  (await browser.execute(() =>
    Array.from(document.querySelectorAll('[data-toast-region] [role="status"] p:first-child')).map(
      (p) => p.textContent ?? "",
    ),
  )) as string[];

async function modeIs(name: string): Promise<boolean> {
  const on = await browser.execute(() =>
    Array.from(document.querySelectorAll('[role="radio"][aria-checked="true"]')).map((el) =>
      el.textContent?.trim(),
    ),
  );
  return on.includes(name);
}

// E2E-M5-9 Onboarding (F-SH-08, DESIGN §5.3; F-TL-01 "says once").
describe("E2E-M5-9 onboarding", () => {
  after(async () => {
    await invoke("onboarding_set", { done: true }).catch(() => undefined);
    await invoke("settings_set", { key: "first_finish", value: "shown" }).catch(() => undefined);
  });

  it("F-SH-08.1 Welcome → offline → Engines (its own sign-in) → a folder → Done → Start Claude Code in it lands in Code mode with the agent launching", async function () {
    this.timeout(180_000);
    await freshProfile();
    // Welcome is the sign-in screen, under the rings (DESIGN §5.3.1).
    await $("#signin-title").waitForDisplayed({ timeout: 20_000 });
    expect(await $('[data-testid="welcome-rings"]').isExisting()).toBe(true);
    await $("button=Use Vibezeo offline").click();
    await atStep("engines");
    expect(await textOf('[data-testid="onboarding"] h1')).toBe("Your coding agents");
    // Three steps, not five.
    expect(
      await browser.execute(
        () =>
          document.querySelectorAll('[data-testid="onboarding"] nav[aria-label="Setup progress"] li').length,
      ),
    ).toBe(3);
    // The fake Claude Code is found, with its own sign-in (F-AC-02): a first
    // run has no engine accounts yet.
    await $('[data-testid="onboarding-engines"] [data-engine="claude"]').waitForExist({ timeout: 15_000 });
    await $('[data-testid="onboarding-sign-in-claude"]').waitForExist({ timeout: 15_000 });
    expect(await $('[data-testid="onboarding-sign-in-claude"]').getAttribute("aria-label")).toBe(
      "Sign in to Claude Code",
    );
    expect(
      await $('[data-testid="onboarding-engines"] [data-engine="claude"]').getAttribute("data-default"),
    ).toBe("true");
    await expectNoSeriousA11yViolations();
    // It opens the engine's own sign-in as a new account, named first.
    await $('[data-testid="onboarding-sign-in-claude"]').click();
    await $('[data-testid="login-nickname"]').waitForExist({ timeout: 5_000 });
    expect(await $('[data-testid="login-nickname"]').getValue()).toBe("Personal");
    await browser.keys("Escape");
    await $('[data-testid="login-nickname"]').waitForExist({ reverse: true, timeout: 5_000 });

    await $('[data-testid="onboarding-continue"]').click();
    await atStep("workspace");
    expect(await textOf('[data-testid="onboarding"] h1')).toBe("Pick a project folder");
    // F-SH-08.2: quitting at step 3 (counting Welcome) resumes there.
    await relaunch();
    await atStep("workspace");
    await expectNoSeriousA11yViolations();

    const folder = pickFolder(`/tmp/tandem-e2e-m5-9-${Date.now()}`);
    const name = basename(folder);
    await $('[data-testid="onboarding-choose-folder"]').click();
    await atStep("finish");
    expect(await textOf('[data-testid="onboarding"] h1')).toBe("You're set");
    // The privacy switches are one line now, both off (F-TL-01).
    expect(await textOf('[data-testid="onboarding-privacy"]')).toBe(
      "Usage statistics and crash reports stay off. You can turn them on in Settings › Privacy.",
    );
    expect(await $('[data-testid="onboarding"] [role="switch"]').isExisting()).toBe(false);
    await expectNoSeriousA11yViolations();
    // The button leads with the engine's glyph.
    expect(await textOf('[data-testid="onboarding-start"]')).toMatch(
      new RegExp(`Start Claude Code in ${name}$`),
    );
    expect(await textOf('[data-testid="onboarding-skip"]')).toBe(`Open ${name} without an agent`);

    await $('[data-testid="onboarding-start"]').click();
    await $('[data-testid="app-shell"]').waitForExist({ timeout: 20_000 });
    expect(await $('[data-testid="onboarding"]').isExisting()).toBe(false);
    expect(await modeIs("Code")).toBe(true);
    expect(await workspaceRoots()).toEqual([folder]);
    // The agent launches through the launch grid's path, alone in the first tab.
    await browser.waitUntil(async () => (await paneIds()).length === 1, {
      timeout: 20_000,
      timeoutMsg: "no agent pane",
    });
    const [agent] = await paneIds();
    await waitForPaneText(agent ?? "", "Fake Claude Code", 20_000);
    expect(await $('[data-testid="launch-grid"]').isExisting()).toBe(false);
    const settings = await invoke<Record<string, unknown>>("settings_get_all");
    expect(settings.telemetry_usage).toBe(false);
    expect(settings.telemetry_crash).toBe(false);
    expect(settings.first_finish).toBe("armed");
    expect((await invoke<{ done: boolean }>("onboarding_get")).done).toBe(true);

    // F-SH-08.5: its first finish brings one toast, once.
    await sendToPane(agent ?? "", "hello\r");
    await waitForStatus(agent ?? "", "done", 20_000);
    await browser.waitUntil(async () => (await toasts()).includes(`Claude Code finished in ${name}`), {
      timeout: 10_000,
      timeoutMsg: `no first-finish toast: ${JSON.stringify(await toasts())}`,
    });
    expect((await invoke<Record<string, unknown>>("settings_get_all")).first_finish).toBe("shown");
    // The folder isn't a git repository: the action opens the pane.
    expect(await $("button=Open it").isExisting()).toBe(true);
    await $("button=Open it").click();
    await browser.waitUntil(async () => !(await toasts()).some((t) => t.includes("finished in")), {
      timeout: 5_000,
    });
    // A second finish brings nothing.
    await sendToPane(agent ?? "", "again\r");
    await waitForStatus(agent ?? "", "working", 10_000).catch(() => undefined);
    await waitForStatus(agent ?? "", "done", 20_000);
    await browser.pause(1_000);
    expect((await toasts()).some((t) => t.includes("finished in"))).toBe(false);
  });

  it("F-SH-08.3 Run setup again, then skipping every step, lands in an empty Code mode that invites a folder", async function () {
    this.timeout(60_000);
    for (const w of await invoke<Array<{ id: string }>>("workspace_list"))
      await invoke("workspace_remove", { id: w.id });
    await browser.refresh();
    await $('[data-testid="app-shell"]').waitForExist({ timeout: 20_000 });
    // Settings › General → "Run setup again" (Mod+, opens Settings).
    await browser.execute(() =>
      (
        document.querySelector(
          'nav[aria-label="Workspaces"] button[aria-label="Add folder…"]',
        ) as HTMLElement | null
      )?.focus(),
    );
    await browser.keys([mod(), ","]);
    await $('nav[aria-label="Settings pages"]').waitForExist({ timeout: 5_000 });
    await typeKeys("setup");
    await $('[data-testid="run-setup-again"]').waitForExist({ timeout: 10_000 });
    await $('[data-testid="run-setup-again"]').click();
    for (const s of ["engines", "workspace", "finish"]) {
      await atStep(s);
      await $('[data-testid="onboarding-skip"]').click();
    }
    await browser.waitUntil(async () => !(await $('[data-testid="onboarding"]').isExisting()), {
      timeout: 10_000,
    });
    await browser.waitUntil(async () => (await textOf("main h2, main h1")).includes("Add a project folder"), {
      timeout: 10_000,
      timeoutMsg: "no empty state",
    });
  });
});
