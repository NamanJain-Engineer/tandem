import { $, browser } from "@wdio/globals";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { checkAgain, fakeEngine, freshOffline, home, invoke } from "../support/app";
import { assistantTexts, composer, sendMessage, threadMode, waitForAssistant } from "../support/thread";

type Detail = { thread: { engineId: string }; turns: Array<{ engineId: string }> };

async function transcript(): Promise<string> {
  return browser.execute(
    () => document.querySelector('[data-testid="thread-main"] [role="log"]')?.textContent ?? "",
  );
}

// E2E-M7-8 Switch engine (F-TH-16): after 3 turns on Claude Code, switching
// to Codex starts a Codex session whose first input carries the summary, and
// the transcript shows the divider.
describe("E2E-M7-8 switch engine", () => {
  let threadId = "";

  before(async function () {
    this.timeout(60_000);
    await freshOffline();
    fakeEngine("codex", join(home(), "fakebin"));
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).toContain("codex");
    await threadMode();
    threadId = (
      await invoke<{ id: string }>("thread_create", {
        args: { kind: "thread", workspaceId: null, engineId: "claude" },
      })
    ).id;
    const row = await $(`[data-testid="thread-row"][data-thread-id="${threadId}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await composer().waitForDisplayed({ timeout: 10_000 });
  });

  // Later specs run without this Codex.
  after(async () => {
    rmSync(join(home(), "fakebin", "codex"), { force: true });
    const report = await checkAgain();
    expect(report.found.map((f) => f.engineId)).not.toContain("codex");
  });

  it("F-TH-16.1 Claude → Codex after 3 turns seeds Codex with the summary and shows the divider", async function () {
    this.timeout(120_000);
    for (const word of ["alpha", "bravo", "charlie"]) {
      await sendMessage(word);
      await waitForAssistant(`You said: ${word}`);
    }

    // The composer's engine picker.
    await $('[data-testid="composer"] [aria-label="Engine"]').click();
    const codex = $(`//*[@role="option"][.//*[normalize-space()="Codex"]]`);
    await codex.waitForClickable({ timeout: 10_000 });
    await codex.click();
    await browser.waitUntil(
      async () => (await invoke<Detail>("thread_get", { threadId })).thread.engineId === "codex",
      { timeout: 10_000, timeoutMsg: "the thread didn't switch to Codex" },
    );

    await sendMessage("delta");
    // The fake engine echoes its whole input: the summary, then the message.
    await browser.waitUntil(
      async () =>
        (await assistantTexts()).some((t) =>
          t.includes("You said: This conversation began with another coding agent"),
        ),
      { timeout: 30_000, timeoutMsg: "Codex never answered with the summary" },
    );
    const first = (await assistantTexts()).at(-1) ?? "";
    for (const said of ["User: alpha", "User: bravo", "User: charlie", "Assistant: You said: charlie"])
      expect(first).toContain(said);
    expect(first.trimEnd().endsWith("delta")).toBe(true);
    await browser.waitUntil(
      async () => (await transcript()).includes("Continued on Codex (Default) with a summary"),
      { timeout: 10_000, timeoutMsg: "no divider" },
    );
    // The summary is the engine's input, not a message in the transcript.
    const users = await browser.execute(() =>
      Array.from(document.querySelectorAll<HTMLElement>('[data-testid="thread-user"]')).map((e) =>
        e.innerText.trim(),
      ),
    );
    expect(users.at(-1)).toBe("delta");

    // Turns record their engine, so usage adds up per engine.
    const detail = await invoke<Detail>("thread_get", { threadId });
    expect(detail.turns.map((t) => t.engineId)).toEqual(["claude", "claude", "claude", "codex"]);

    // The next Codex turn is plain.
    await sendMessage("echo");
    await waitForAssistant("You said: echo");
    expect((await assistantTexts()).at(-1)).toBe("You said: echo");
  });
});
