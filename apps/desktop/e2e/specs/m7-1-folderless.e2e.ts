import { $, browser } from "@wdio/globals";
import { freshOffline, mod } from "../support/app";
import { headerTitle, railRows, sendMessage, threadMode, waitForAssistant } from "../support/thread";

// E2E-M7-1 Folderless (F-TH-02, F-TH-15): a fresh profile in Thread mode has
// a composer; "hello" starts a thread under No folder with a streamed reply
// and a title (the local one: AI titles are opt-in, D-118, and this run is
// offline); a rename sticks through more turns.
describe("E2E-M7-1 folderless", () => {
  it("F-TH-02.1 F-TH-15.1 F-TH-15.2 hello starts a No-folder thread with a reply and a title; a rename sticks", async function () {
    this.timeout(120_000);
    await freshOffline();
    await threadMode();
    expect(await $("h2=Start without a folder").isExisting()).toBe(true);

    const sent = Date.now();
    await sendMessage("hello");
    // The local title appears with the message, well within 2 s.
    await browser.waitUntil(async () => (await railRows()).some((r) => r.title === "Hello"), {
      timeout: 2_000,
      timeoutMsg: `rail: ${JSON.stringify(await railRows())}`,
    });
    expect(Date.now() - sent).toBeLessThan(2_000);
    expect(await railRows()).toEqual([{ title: "Hello", group: "No folder" }]);
    await waitForAssistant("You said: hello");
    expect(await headerTitle()).toBe("Hello");

    // Rename from the header; more turns keep the name.
    await $('[data-testid="thread-title"]').click();
    const field = await $('header input[aria-label^="New name for"]');
    await field.waitForDisplayed({ timeout: 5_000 });
    // Typed, not setValue: clearing first would blur the field, which saves.
    await browser.keys([mod(), "a"]);
    await browser.keys("Release notes".split(""));
    await browser.keys("Enter");
    await browser.waitUntil(async () => (await headerTitle()) === "Release notes", { timeout: 5_000 });
    await sendMessage("and the changelog");
    await waitForAssistant("You said: and the changelog");
    await sendMessage("thanks");
    await waitForAssistant("You said: thanks");
    await browser.pause(500);
    expect(await headerTitle()).toBe("Release notes");
    expect((await railRows()).map((r) => r.title)).toEqual(["Release notes"]);
  });
});
