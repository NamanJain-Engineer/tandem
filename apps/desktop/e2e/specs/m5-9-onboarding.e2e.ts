import { $, browser } from "@wdio/globals";
import { expectNoSeriousA11yViolations } from "../support/axe";
import {
  freshProfile,
  invoke,
  mod,
  paneIds,
  pickFolder,
  relaunch,
  textOf,
  typeKeys,
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
const checked = (name: string) =>
  $(`[data-testid="onboarding"] [role="switch"][aria-label="${name}"]`).getAttribute("aria-checked");

async function modeIs(name: string): Promise<boolean> {
  const on = await browser.execute(() =>
    Array.from(document.querySelectorAll('[role="radio"][aria-checked="true"]')).map((el) =>
      el.textContent?.trim(),
    ),
  );
  return on.includes(name);
}

// E2E-M5-9 Onboarding framework (F-SH-08 slice, F-TL-01 "asks once").
describe("E2E-M5-9 onboarding", () => {
  after(async () => {
    await invoke("onboarding_set", { done: true }).catch(() => undefined);
  });

  it("F-SH-08.1 Welcome → offline → Engines (skip) → Accounts → Privacy (both off) → a folder lands in Code mode on the launch grid", async function () {
    this.timeout(120_000);
    await freshProfile();
    // Welcome is the sign-in screen.
    await $("#signin-title").waitForDisplayed({ timeout: 20_000 });
    await $("button=Use Vibezeo offline").click();
    await atStep("engines");
    expect(await textOf('[data-testid="onboarding"] h1')).toBe("Set up your coding agents");
    await expectNoSeriousA11yViolations();
    await $('[data-testid="onboarding-skip"]').click();
    // Step 3, Accounts: the installed chat engine's own sign-in (F-AC-02).
    await atStep("accounts");
    expect(await textOf('[data-testid="onboarding"] h1')).toBe("Sign in to your agents");
    await $('[data-testid="onboarding-sign-in-claude"]').waitForExist({ timeout: 15_000 });
    expect(await textOf('[data-testid="onboarding-sign-in-claude"]')).toBe("Sign in to Claude Code");
    await expectNoSeriousA11yViolations();
    // It opens the engine's own sign-in as a new account, named first.
    await $('[data-testid="onboarding-sign-in-claude"]').click();
    await $('[data-testid="login-nickname"]').waitForExist({ timeout: 5_000 });
    expect(await $('[data-testid="login-nickname"]').getValue()).toBe("Personal");
    await browser.keys("Escape");
    await $('[data-testid="login-nickname"]').waitForExist({ reverse: true, timeout: 5_000 });

    // F-SH-08.2: quitting at step 3 resumes at step 3.
    await relaunch();
    await atStep("accounts");

    await $('[data-testid="onboarding-continue"]').click();
    await atStep("privacy");
    expect(await checked("Share anonymous usage statistics")).toBe("false");
    expect(await checked("Send crash reports")).toBe("false");
    await expectNoSeriousA11yViolations();

    await $('[data-testid="onboarding-continue"]').click();
    await atStep("workspace");
    await expectNoSeriousA11yViolations();
    const folder = pickFolder(`/tmp/tandem-e2e-m5-9-${Date.now()}`);
    await $('[data-testid="onboarding-choose-folder"]').click();
    await $('[data-testid="launch-grid"]').waitForDisplayed({ timeout: 20_000 });
    expect(await $('[data-testid="onboarding"]').isExisting()).toBe(false);
    expect(await modeIs("Code")).toBe(true);
    expect(await workspaceRoots()).toEqual([folder]);
    expect(await paneIds()).toEqual([]);
    const settings = await invoke<Record<string, unknown>>("settings_get_all");
    expect(settings.telemetry_usage).toBe(false);
    expect(settings.telemetry_crash).toBe(false);
    expect((await invoke<{ done: boolean }>("onboarding_get")).done).toBe(true);
  });

  it("F-SH-08.3 Run setup again, then skipping every step, lands in an empty Code mode that invites a folder", async function () {
    this.timeout(60_000);
    for (const w of await invoke<Array<{ id: string }>>("workspace_list"))
      await invoke("workspace_remove", { id: w.id });
    await browser.refresh();
    await $('[data-testid="plan-footer"]').waitForExist({ timeout: 20_000 });
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
    for (const s of ["engines", "accounts", "privacy", "workspace"]) {
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
