import { $, browser } from "@wdio/globals";
import { addFolder, freshOffline, paneIds, rawRead, waitForPaneText } from "../support/app";

// E2E-M3-11 IME (F-CD-07 AC2): composition happens in xterm's textarea and
// the composed text is sent once, on commit; an Enter that belongs to the
// IME sends nothing. The composition events are synthetic (WebDriver can't
// drive an IME); real IMEs on macOS and Windows are in HUMAN_TODO.
describe("E2E-M3-11 IME", () => {
  it("F-CD-07.2 composing にほんご then Enter sends its UTF-8 once and no CR during composition", async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m3-11-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const pane = (await paneIds())[0] ?? "";
    await waitForPaneText(pane, "#");
    const read = await rawRead(pane, 13, 15);
    await $(`[data-testid="terminal"][data-pane-id="${pane}"]`).click();
    await browser.executeAsync((done: () => void) => {
      const ta = document.activeElement as HTMLTextAreaElement;
      const fire = (e: Event) => ta.dispatchEvent(e);
      fire(new CompositionEvent("compositionstart", { bubbles: true, data: "" }));
      for (const step of ["に", "にほ", "にほん", "にほんご"]) {
        ta.value = step;
        fire(new CompositionEvent("compositionupdate", { bubbles: true, data: step }));
        fire(
          new InputEvent("input", {
            bubbles: true,
            data: step,
            inputType: "insertCompositionText",
            isComposing: true,
          }),
        );
      }
      // The Enter that commits the IME's choice belongs to the IME.
      fire(
        new KeyboardEvent("keydown", {
          bubbles: true,
          cancelable: true,
          key: "Enter",
          code: "Enter",
          keyCode: 229,
          isComposing: true,
        }),
      );
      fire(new CompositionEvent("compositionend", { bubbles: true, data: "にほんご" }));
      fire(new KeyboardEvent("keyup", { bubbles: true, key: "Enter", code: "Enter", keyCode: 13 }));
      setTimeout(done, 200);
    });
    // Now a plain Enter submits.
    await browser.keys("Enter");
    expect(await read()).toBe("e3 81 ab e3 81 bb e3 82 93 e3 81 94 0d");
  });
});
