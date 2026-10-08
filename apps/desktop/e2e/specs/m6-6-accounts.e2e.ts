import { $, browser } from "@wdio/globals";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshUser, home, invoke, mod, paneIds, planFooter, setPlan, split } from "../support/app";

type Account = { id: string; engineId: string; label: string; homePath: string; isDefault: boolean };
type Pane = { id: string; accountId: string | null };

const claudeAccounts = async () =>
  (await invoke<Account[]>("accounts_list")).filter((a) => a.engineId === "claude");

/** Processes whose environment points at `dir` (the login runs there). */
function processesIn(dir: string): number[] {
  const pids: number[] = [];
  for (const entry of readdirSync("/proc")) {
    if (!/^\d+$/.test(entry)) continue;
    try {
      if (readFileSync(`/proc/${entry}/environ`, "utf8").split("\0").includes(`CLAUDE_CONFIG_DIR=${dir}`))
        pids.push(Number(entry));
    } catch {
      // Gone, or not ours.
    }
  }
  return pids;
}

const miniText = () =>
  browser.execute(
    () => document.querySelector('[data-testid="mini-terminal"] .xterm-rows')?.textContent ?? "",
  );

async function openAccounts(): Promise<void> {
  if (!(await $('[data-testid="accounts-page"]').isExisting())) {
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await browser.keys([mod(), ","]);
    await $('nav[aria-label="Settings pages"]').waitForExist({ timeout: 5_000 });
    await $('//nav[@aria-label="Settings pages"]//*[normalize-space()="Accounts"]').click();
  }
  await $('[data-testid="accounts-page"]').waitForExist({ timeout: 10_000 });
}

async function startAdding(nickname: string): Promise<void> {
  await $('[data-testid="account-add"]').click();
  await $('[data-testid="login-nickname"]').setValue(nickname);
  await $('[data-testid="login-start"]').click();
}

async function closeSettings(): Promise<void> {
  await browser.keys("Escape");
  await $('[data-testid="accounts-page"]').waitForExist({ reverse: true, timeout: 5_000 });
}

// E2E-M6-6 Accounts (F-AC-02, F-AC-03, UI-35).
describe("E2E-M6-6 accounts", () => {
  let email = "";
  let tabId = "";
  let first: Pane = { id: "", accountId: null };

  it("F-AC-02.1 cancelling mid-login kills the login process and creates no account", async function () {
    this.timeout(120_000);
    ({ email } = await freshUser("m6-6"));
    await setPlan(email, "pro");
    await browser.waitUntil(async () => (await planFooter()).startsWith("PRO"), { timeout: 20_000 });
    await addFolder(`/tmp/tandem-e2e-m6-6-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [ws] = await invoke<Array<{ id: string; tabs: Array<{ id: string }> }>>("workspace_list");
    tabId = ws?.tabs[0]?.id ?? "";
    // A Claude pane on the engine's default account, opened before any change.
    first = await invoke<Pane>("pane_create", { tabId, kind: "agent", engineId: "claude", cwd: null });
    expect(first.accountId).toBe((await claudeAccounts()).find((a) => a.isDefault)?.id);

    await openAccounts();
    await startAdding("Halfway");
    const sheet = $('[data-testid="login-sheet"]');
    await browser.waitUntil(async () => Boolean(await sheet.getAttribute("data-account-id")), {
      timeout: 15_000,
    });
    const dir = join(home(), "providers", "claude", (await sheet.getAttribute("data-account-id")) ?? "");
    // The engine's own login is waiting for its code.
    await browser.waitUntil(async () => (await miniText()).includes("Paste the code to finish"), {
      timeout: 30_000,
      timeoutMsg: `terminal: ${await miniText()}`,
    });
    expect(processesIn(dir).length).toBeGreaterThan(0);
    await $('[data-testid="login-cancel"]').click();
    await browser.waitUntil(async () => processesIn(dir).length === 0, {
      timeout: 10_000,
      timeoutMsg: `left running: ${processesIn(dir).join(", ")}`,
    });
    await browser.waitUntil(async () => !existsSync(dir), { timeout: 5_000 });
    expect((await claudeAccounts()).map((a) => a.label)).not.toContain("Halfway");
  });

  it("F-AC-02.2 a second account signs in; Make default sends the next pane there, and the open one keeps its own", async function () {
    this.timeout(120_000);
    await openAccounts();
    await startAdding("Personal");
    await browser.waitUntil(async () => (await miniText()).includes("Paste the code to finish"), {
      timeout: 30_000,
    });
    await $('[data-testid="mini-terminal"]').click();
    await browser.keys(["4", "2", "Enter"]);
    await $('//*[@data-testid="login-sheet"]//*[normalize-space()="Signed in as Personal"]').waitForExist({
      timeout: 20_000,
    });
    await $('[data-testid="login-done"]').click();
    const personal = (await claudeAccounts()).find((a) => a.label === "Personal");
    expect(personal).toBeDefined();
    // The sign-in lives in its own home, which Vibezeo checks but never reads.
    expect(existsSync(join(personal?.homePath ?? "", ".credentials.json"))).toBe(true);

    await $(
      `[data-testid="account-row"][data-label="Personal"] [data-testid="account-make-default"]`,
    ).click();
    await browser.waitUntil(
      async () => (await claudeAccounts()).find((a) => a.isDefault)?.label === "Personal",
      { timeout: 10_000 },
    );
    const next = await invoke<Pane>("pane_create", { tabId, kind: "agent", engineId: "claude", cwd: null });
    expect(next.accountId).toBe(personal?.id);
    const panes = await invoke<Pane[]>("pane_list", { tabId });
    expect(panes.find((p) => p.id === first.id)?.accountId).toBe(first.accountId);
    await invoke("pane_close", { paneId: next.id });
    await invoke("pane_close", { paneId: first.id });
  });

  it("F-AC-03 the AccountChip sends new panes in this workspace to another account; the default stays", async function () {
    this.timeout(120_000);
    await closeSettings();
    const before = (await claudeAccounts()).find((a) => a.isDefault);
    const other = (await claudeAccounts()).find((a) => !a.isDefault);
    const agent = await split("right", "claude");
    const chip = $(`[data-testid="pane"][data-pane-id="${agent}"] [data-testid="account-chip"]`);
    expect(await chip.getProperty("textContent")).toBe(before?.label);
    await chip.click();
    await browser.waitUntil(
      async () =>
        browser.execute((label: string) => {
          const item = Array.from(document.querySelectorAll<HTMLElement>("[role=menuitemcheckbox]")).find(
            (el) => el.textContent?.includes(`Use ${label} for new`),
          );
          item?.click();
          return item !== undefined;
        }, other?.label ?? ""),
      { timeout: 5_000 },
    );
    await browser.waitUntil(async () => (await invoke<unknown[]>("accounts_overrides")).length === 1, {
      timeout: 5_000,
    });
    const after = await invoke<Pane>("pane_create", { tabId, kind: "agent", engineId: "claude", cwd: null });
    expect(after.accountId).toBe(other?.id);
    expect((await claudeAccounts()).find((a) => a.isDefault)?.id).toBe(before?.id);
    // The open pane didn't switch.
    expect(await chip.getProperty("textContent")).toBe(before?.label);
    await invoke("pane_close", { paneId: after.id });
  });

  it("F-AC-02 on Free a second account per engine is blocked with Upgrade", async function () {
    this.timeout(60_000);
    await setPlan(email, "free");
    await invoke("entitlement_refresh");
    await browser.waitUntil(async () => (await planFooter()).startsWith("FREE"), { timeout: 20_000 });
    await openAccounts();
    await startAdding("Third");
    await $('[data-testid="login-upgrade"]').waitForExist({ timeout: 10_000 });
    expect(await $('[data-testid="login-sheet"]').getProperty("textContent")).toContain(
      "Pro lets you add more accounts per engine.",
    );
    await $('[data-testid="login-cancel"]').click();
  });
});
