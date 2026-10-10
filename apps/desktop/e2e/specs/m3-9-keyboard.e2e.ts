import { $, browser } from "@wdio/globals";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  freshOffline,
  invoke,
  mod,
  paneIds,
  rawRead,
  sendToPane,
  typeKeys,
  waitForPaneText,
} from "../support/app";

const root = new URL("../../../..", import.meta.url).pathname;
const COMMANDS = JSON.parse(readFileSync(join(root, "packages/protocol/shortcuts.json"), "utf8")) as Array<{
  id: string;
  enabledWhen: string;
}>;
const EN = JSON.parse(readFileSync(join(root, "apps/desktop/src/i18n/en.json"), "utf8")) as Record<
  string,
  string
>;

/** Thread mode with a thread open, for commands that act on the open thread. */
async function openThread(): Promise<void> {
  const list = await invoke<Array<{ id: string }>>("thread_list", { kind: "thread" });
  const id =
    list[0]?.id ??
    (await invoke<{ id: string }>("thread_create", { args: { kind: "thread", workspaceId: null } })).id;
  await browser.keys([mod(), "3"]);
  await browser.waitUntil(async () => (await mode()) === "Thread", { timeout: 3_000 });
  const row = $(`[data-testid="thread-row"][data-thread-id="${id}"]`);
  await row.waitForClickable({ timeout: 10_000 });
  await row.click();
  await $(`[data-testid="thread-main"][data-thread-id="${id}"]`).waitForExist({ timeout: 10_000 });
}

async function focusTerminal(pane: string): Promise<void> {
  await $(`[data-testid="terminal"][data-pane-id="${pane}"]`).click();
}

async function mode(): Promise<string> {
  return String(
    await browser.execute(
      () =>
        document
          .querySelector<HTMLElement>(
            '[role="radiogroup"][aria-label="Work mode"] [role="radio"][aria-checked="true"]',
          )
          ?.innerText.trim() ?? "",
    ),
  );
}

async function runs(): Promise<Record<string, number>> {
  return (await browser.execute(() =>
    (
      window as unknown as { __tandemTest: { commandRuns: () => Record<string, number> } }
    ).__tandemTest.commandRuns(),
  )) as Record<string, number>;
}

// E2E-M3-9 Keyboard (F-SH-09, F-CD-05 AC1).
describe("E2E-M3-9 keyboard", function () {
  // F-SH-09.4 runs every registry command through the palette, about 1.5 s
  // each; the budget grows with the registry (D-821). Each command still has
  // its own 3 s to run.
  this.timeout(60_000 + COMMANDS.length * 3_000);
  let pane = "";

  before(async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m3-9-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    pane = (await paneIds())[0] ?? "";
    await waitForPaneText(pane, "#");
  });

  it("F-SH-09.1 Ctrl+W/T/D/[/] in a terminal reach the PTY as 17 14 04 1b 1d", async () => {
    const read = await rawRead(pane, 5);
    await focusTerminal(pane);
    for (const k of ["w", "t", "d", "[", "]"]) await browser.keys(["Control", k]);
    expect(await read()).toBe("17 14 04 1b 1d");
    expect(await paneIds()).toEqual([pane]);
    expect(await browser.execute(() => document.querySelectorAll('[data-testid="tab"]').length)).toBe(1);
  });

  it("F-SH-09.2 Ctrl+Shift+D splits and sends nothing", async () => {
    const read = await rawRead(pane, 1, 3);
    await focusTerminal(pane);
    await browser.keys(["Control", "Shift", "d"]);
    await $('[data-testid="pane-picker"]').waitForDisplayed({ timeout: 5_000 });
    await browser.keys("Escape");
    expect(await read()).toBe("");
  });

  it("F-CD-05.1 Ctrl+C sends 0x03 without a selection and copies (sending nothing) with one", async () => {
    let read = await rawRead(pane, 1);
    await focusTerminal(pane);
    await browser.keys(["Control", "c"]);
    expect(await read()).toBe("03");
    await sendToPane(pane, "clear; printf 'copy''-me-9\\n'\n");
    await waitForPaneText(pane, "copy-me-9");
    read = await rawRead(pane, 1, 3);
    await focusTerminal(pane);
    await browser.execute(
      (id: string) =>
        (window as unknown as { __tandemTest: { selectAll: (p: string) => void } }).__tandemTest.selectAll(
          id,
        ),
      pane,
    );
    await browser.keys(["Control", "c"]);
    await browser.waitUntil(
      async () => String(await invoke("plugin:clipboard-manager|read_text")).includes("copy-me-9"),
      {
        timeout: 5_000,
        timeoutMsg: "the selection wasn't copied",
      },
    );
    expect(await read()).toBe("");
  });

  it("F-SH-09.3 F5 and Ctrl+R don't reload the window", async () => {
    await browser.execute(() => {
      (window as unknown as { __noReload: boolean }).__noReload = true;
      (document.activeElement as HTMLElement | null)?.blur();
    });
    await browser.keys("F5");
    await browser.keys(["Control", "r"]);
    await browser.pause(1_000);
    expect(
      await browser.execute(() => (window as unknown as { __noReload?: boolean }).__noReload === true),
    ).toBe(true);
  });

  it("F-SH-09.5 Mod+1 during composition doesn't switch mode", async () => {
    await focusTerminal(pane);
    await browser.execute(() =>
      document.activeElement?.dispatchEvent(
        new CompositionEvent("compositionstart", { bubbles: true, data: "" }),
      ),
    );
    await browser.keys([mod(), "1"]);
    await browser.pause(300);
    expect(await mode()).toBe("Code");
    await browser.execute(() =>
      document.activeElement?.dispatchEvent(
        new CompositionEvent("compositionend", { bubbles: true, data: "" }),
      ),
    );
    await browser.keys([mod(), "1"]);
    await browser.waitUntil(async () => (await mode()) === "Agent", { timeout: 3_000 });
    await browser.keys([mod(), "2"]);
    await browser.waitUntil(async () => (await mode()) === "Code", { timeout: 3_000 });
  });

  it("F-SH-09.4 every registry command runs from the palette through its handler", async () => {
    const before = await runs();
    await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
    await browser.keys([mod(), "k"]);
    await $('input[placeholder="Type a command or search…"]').waitForDisplayed({ timeout: 3_000 });
    await browser.keys("Escape");
    expect((await runs())["palette.open"]).toBe((before["palette.open"] ?? 0) + 1);
    // Closing the pane last keeps a focused pane for the rest.
    // Moving and closing the pane come last: the rest need it focused.
    const last = ["pane.move_to_new_tab", "pane.close"];
    const ids = COMMANDS.map((c) => c.id).filter((id) => id !== "palette.open" && !last.includes(id));
    ids.push(...last);
    for (const id of ids) {
      const was = (await runs())[id] ?? 0;
      if (COMMANDS.find((c) => c.id === id)?.enabledWhen === "thread_open") await openThread();
      await browser.execute(() => (document.activeElement as HTMLElement | null)?.blur());
      await browser.keys([mod(), "k"]);
      const input = $('input[placeholder="Type a command or search…"]');
      await input.waitForDisplayed({ timeout: 3_000 });
      await typeKeys(EN[`cmd.${id}`] ?? id);
      await browser.keys("Enter");
      // Plan-gated commands are absent on this Free profile (F-RT-04): Enter
      // runs nothing.
      if (COMMANDS.find((c) => c.id === id)?.enabledWhen === "routines") {
        await browser.pause(500);
        expect((await runs())[id] ?? 0).toBe(was);
      } else {
        await browser.waitUntil(async () => ((await runs())[id] ?? 0) === was + 1, {
          timeout: 3_000,
          timeoutMsg: `${id} didn't run from the palette`,
        });
      }
      // Close whatever it opened (dialog, picker, find bar, Settings) and come back to Code.
      for (let i = 0; i < 2; i++) {
        await browser.keys("Escape");
        await browser.pause(150);
      }
      if ((await mode()) !== "Code") {
        await browser.keys([mod(), "2"]);
        await browser.waitUntil(async () => (await mode()) === "Code", { timeout: 3_000 });
      }
    }
  });
});
