import { browser } from "@wdio/globals";
import { addFolder, fakeLog, freshOffline, invoke, paneIds, sendToPane, split, textOf, waitForPaneText, waitForStatus } from "../support/app";

const hex = (s: string) => Buffer.from(s, "utf8").toString("hex");

// E2E-M4-5 Typing (F-EN-06): one bracketed paste, then Enter as its own write.
describe("E2E-M4-5 typing", () => {
  it("F-EN-06.1 type_prompt('hello\\x03world') sends helloworld, then CR, as two writes", async () => {
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m4-5-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const agent = await split("right", "claude");
    await waitForPaneText(agent, "Fake Claude Code");
    const mark = fakeLog().length;
    await invoke("pty_type_prompt", { paneId: agent, text: "hello\u0003world" });
    await waitForPaneText(agent, "you said: helloworld");
    const raw = fakeLog()
      .slice(mark)
      .filter((l) => l.startsWith("raw: "))
      .map((l) => l.slice(5));
    // The TUI turned bracketed paste on, so the text arrives as one paste.
    expect(raw[0]).toBe(hex("\u001b[200~helloworld\u001b[201~"));
    expect(raw[1]).toBe("0d");
  });

  it("F-EN-06.2 a pane waiting for approval refuses a typed prompt with the message", async () => {
    const [, agent] = await paneIds();
    await sendToPane(agent ?? "", "/ask\r");
    await waitForStatus(agent ?? "", "waiting");
    const err = await invoke("pty_type_prompt", { paneId: agent, text: "go on" }).then(
      () => "",
      (e: Error) => e.message,
    );
    // "<name> is waiting for your approval": the pane's first name (F-CD-12).
    const name = await textOf(`[data-testid="pane"][data-pane-id="${agent}"] [data-testid="pane-name"]`);
    expect(name).not.toBe("");
    expect(err).toContain(`${name} is waiting for your approval`);
    expect(err).toContain('"code":"waiting"');
  });
});
