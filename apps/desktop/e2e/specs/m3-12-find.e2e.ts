import { $, browser } from "@wdio/globals";
import { addFolder, freshOffline, mod, paneIds, sendToPane, waitForPaneText, waitForShell } from "../support/app";

async function countIs(label: string): Promise<void> {
  const count = $('[data-testid="find-count"]');
  await browser.waitUntil(async () => (await count.getText()) === label, {
    timeout: 5_000,
    timeoutMsg: `find count: ${await count.getText()} (wanted ${label})`,
  });
}

// E2E-M3-12 Find (F-CD-08 AC1).
describe("E2E-M3-12 find", () => {
  it("F-CD-08.1 'needle' at lines 10, 500 and 900 reads 1 of 3; next and previous cycle", async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m3-12-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const pane = (await paneIds())[0] ?? "";
    await waitForShell(pane);
    // The command line spells it `nee""dle`, so only the output matches.
    await sendToPane(pane, `for i in $(seq 1 1000); do case $i in 10|500|900) echo "line $i nee""dle";; *) echo "line $i";; esac; done\n`);
    await waitForPaneText(pane, "line 1000");
    await $(`[data-testid="terminal"][data-pane-id="${pane}"]`).click();
    // Linux under "Terminal first": Ctrl+Shift+F (Ctrl+F stays the shell's).
    await browser.keys([mod(), "Shift", "f"]);
    const input = $('[data-testid="find-input"]');
    await input.waitForDisplayed({ timeout: 5_000 });
    await input.setValue("needle");
    await countIs("1 of 3");
    await browser.keys("Enter");
    await countIs("2 of 3");
    await browser.keys("Enter");
    await countIs("3 of 3");
    await browser.keys("Enter");
    await countIs("1 of 3");
    await browser.keys(["Shift", "Enter"]);
    await countIs("3 of 3");
    await browser.keys([mod(), "g"]);
    await countIs("1 of 3");
    await browser.keys([mod(), "Shift", "g"]);
    await countIs("3 of 3");
    // Case toggle: NEEDLE has no case-sensitive match.
    await $('[data-testid="find-bar"] button[aria-label="Match case"]').click();
    await input.setValue("NEEDLE");
    await countIs("No results");
    await input.click();
    await browser.keys("Escape");
    await $('[data-testid="find-bar"]').waitForExist({ reverse: true, timeout: 5_000 });
    // Focus is back in the terminal.
    expect(await browser.execute(() => !!document.activeElement?.closest(".xterm"))).toBe(true);
  });
});
