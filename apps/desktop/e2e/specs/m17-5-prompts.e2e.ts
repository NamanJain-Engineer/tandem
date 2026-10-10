import { $, browser } from "@wdio/globals";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { addFolder, freshOffline, home, invoke, paneIds, paneText, split } from "../support/app";
import { expectNoSeriousA11yViolationsInBothThemes } from "../support/axe";
import { composer, threadMode, userTexts } from "../support/thread";

// E2E-M17-5 Prompts (F-EX-05): a repository prompt with {{branch}} and
// {{ticket}} and a profile prompt are listed with their source; {{ticket}}
// asks; the text lands in a composer unsent and in a pane with no newline.

function git(cwd: string, ...args: string[]): void {
  execFileSync("git", ["-c", "init.defaultBranch=main", "-c", "user.name=E2E", "-c", "user.email=e2e@example.invalid", "-c", "commit.gpgsign=false", ...args], {
    cwd,
    stdio: "ignore",
  });
}

async function openPrompts(): Promise<void> {
  await browser.execute(() => {
    const mac = navigator.platform.toLowerCase().includes("mac");
    window.dispatchEvent(new KeyboardEvent("keydown", { code: "KeyP", key: "p", ctrlKey: !mac, metaKey: mac, altKey: true, bubbles: true, cancelable: true }));
  });
  await $('[data-testid="quick-picker"]').waitForDisplayed({ timeout: 10_000 });
}

async function rows(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="prompt-row"]')).map((r) => `${r.dataset.title}|${r.dataset.source}`),
  );
}

async function pick(title: string): Promise<void> {
  await $('[data-testid="quick-picker"] input').setValue(title);
  await browser.waitUntil(async () => (await rows()).length === 1, { timeout: 10_000, timeoutMsg: `rows: ${JSON.stringify(await rows())}` });
  await browser.keys("Enter");
}

describe("E2E-M17-5 prompts", () => {
  const repo = `/tmp/tandem-e2e-m17-5-${Date.now()}`;

  before(async function () {
    this.timeout(60_000);
    mkdirSync(join(repo, ".tandem/prompts"), { recursive: true });
    writeFileSync(join(repo, "README.md"), "# shop\n");
    git(repo, "init", "-q");
    git(repo, "add", ".");
    git(repo, "commit", "-qm", "first");
    git(repo, "checkout", "-q", "-b", "feature/login");
    writeFileSync(join(repo, ".tandem/prompts/fix-ticket.md"), "---\ntitle: Fix a ticket\ndescription: From the tracker\n---\nOn {{branch}}, fix {{ticket}}.\n");
    mkdirSync(join(home(), "prompts"), { recursive: true });
    writeFileSync(join(home(), "prompts/explain.md"), "Explain what this folder does.\n");
    await freshOffline();
    await addFolder(repo);
  });

  it("F-EX-05.1 both libraries are listed with their source", async function () {
    this.timeout(30_000);
    await openPrompts();
    await browser.waitUntil(async () => (await rows()).length === 2, { timeout: 10_000, timeoutMsg: `rows: ${JSON.stringify(await rows())}` });
    expect((await rows()).sort()).toEqual(["Fix a ticket|repo", "explain|profile"]);
    await expectNoSeriousA11yViolationsInBothThemes();
    await browser.keys("Escape");
  });

  it("F-EX-05.2 {{ticket}} asks, {{branch}} is filled, and F-EX-05.3 the composer gets it unsent", async function () {
    this.timeout(60_000);
    await threadMode();
    const ws = (await invoke<Array<{ id: string; root: string }>>("workspace_list")).find((w) => w.root === repo);
    const id = (await invoke<{ id: string }>("thread_create", { args: { kind: "thread", workspaceId: ws?.id } })).id;
    await $(`[data-testid="thread-row"][data-thread-id="${id}"]`).waitForExist({ timeout: 10_000 });
    await browser.execute((t: string) => document.querySelector<HTMLElement>(`[data-testid="thread-row"][data-thread-id="${t}"]`)?.click(), id);
    await composer().waitForDisplayed({ timeout: 10_000 });
    await openPrompts();
    await pick("ticket");
    const field = await $('[data-testid="prompt-var"][data-name="ticket"]');
    await field.waitForDisplayed({ timeout: 10_000 });
    await field.setValue("T-42");
    await $('[data-testid="prompt-insert"]').click();
    await browser.waitUntil(async () => (await composer().getValue()).includes("On feature/login, fix T-42."), {
      timeout: 10_000,
      timeoutMsg: `composer: ${await composer().getValue()}`,
    });
    await browser.pause(800);
    expect(await userTexts()).toEqual([]);
  });

  it("F-EX-05.3 a pane gets the prompt with no newline", async function () {
    this.timeout(60_000);
    await browser.execute(() => {
      const mac = navigator.platform.toLowerCase().includes("mac");
      window.dispatchEvent(new KeyboardEvent("keydown", { code: "Digit2", key: "2", ctrlKey: !mac, metaKey: mac, bubbles: true, cancelable: true }));
    });
    await browser.waitUntil(async () => (await paneIds()).length >= 1, { timeout: 20_000 });
    const pane = await split("right");
    await browser.pause(1500);
    await openPrompts();
    await pick("explain");
    await browser.waitUntil(async () => (await paneText(pane)).trimEnd().endsWith("Explain what this folder does."), {
      timeout: 10_000,
      timeoutMsg: `screen: ${await paneText(pane)}`,
    });
    await browser.pause(800);
    // Nothing ran: the line is still at the prompt, with no output after it.
    expect((await paneText(pane)).trimEnd().endsWith("Explain what this folder does.")).toBe(true);
  });
});
