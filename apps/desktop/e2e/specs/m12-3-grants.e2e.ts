import { $, browser } from "@wdio/globals";
import { addFolder, freshOffline, invoke, mod, paneIds } from "../support/app";
import { connectTestPlugin, makeTeammate, sendAndWait } from "../support/agents";
import { threadMode } from "../support/thread";
import { connectGithub, mockReset, openTabNamed } from "../support/plugins";

// E2E-M12-3 Grants (F-PL-05, F-AG-03, F-TH-07): connecting makes a plugin
// available, not usable. A new teammate has none of the three connected;
// the nudge's Enable grants GitHub to it; a thread gets GitHub from Attach ▾
// → "Plugins for this thread" and can then call it.
describe("E2E-M12-3 grants", () => {
  let github = "";

  before(async function () {
    this.timeout(90_000);
    await mockReset();
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m12-3-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    github = await connectGithub();
    await connectTestPlugin("test_echo");
    await connectTestPlugin("test_upper");
  });

  let scout = { id: "" };

  it("F-PL-05.1 a new teammate has no grants with three plugins connected", async () => {
    scout = await makeTeammate("Scout");
    expect(
      await invoke<string[]>("grants_list", { subject: { type: "teammate", teammateId: scout.id } }),
    ).toEqual([]);
  });

  it("F-AG-03 the nudge's Enable grants GitHub to the teammate", async function () {
    this.timeout(60_000);
    // Free has one teammate besides the Lead: the one just made.
    const rhea = scout;
    await browser.keys([mod(), "1"]);
    await $(`[data-teammate-id="${rhea.id}"]`).click();
    await $('[data-testid="teammate-page"]').waitForDisplayed({ timeout: 10_000 });
    await openTabNamed('[data-testid="teammate-page"]', "Chats");
    const nudge = $('[data-testid="plugin-nudge"]');
    await nudge.waitForDisplayed({ timeout: 10_000 });
    expect(await nudge.getText()).toContain("GitHub");
    await nudge.$("button=Enable").click();
    await nudge.waitForExist({ reverse: true, timeout: 10_000 });
    const granted = await invoke<string[]>("grants_list", {
      subject: { type: "teammate", teammateId: rhea.id },
    });
    expect(granted).toContain(github);
  });

  it("F-TH-07 Plugins for this thread grants GitHub to that thread only", async function () {
    this.timeout(90_000);
    await threadMode();
    const [ws] = await invoke<Array<{ id: string }>>("workspace_list");
    const thread = await invoke<{ id: string }>("thread_create", {
      args: { kind: "thread", workspaceId: ws?.id, engineId: "claude" },
    });
    const other = await invoke<{ id: string }>("thread_create", {
      args: { kind: "thread", workspaceId: ws?.id, engineId: "claude" },
    });
    const row = $(`[data-testid="thread-row"][data-thread-id="${thread.id}"]`);
    await row.waitForExist({ timeout: 10_000 });
    await row.click();
    await $('[data-testid="thread-main"]').waitForExist({ timeout: 10_000 });
    // Default none.
    const refused = await sendAndWait(
      thread.id,
      'mcp tandem__call {"tool":"github__get_me","args":{}}',
      60_000,
    );
    expect(refused.join(" ")).not.toContain("octo-shop");

    await $('[data-testid="composer"] button[aria-label="Attach"]').click();
    await $('[role="menuitem"]=Plugins for this thread…').click();
    const item = $('[data-testid="thread-plugin-github"]');
    await item.waitForDisplayed({ timeout: 10_000 });
    const sw = item.$('[role="switch"]');
    expect(await sw.getAttribute("aria-checked")).toBe("false");
    await sw.click();
    await browser.waitUntil(async () => (await sw.getAttribute("aria-checked")) === "true", {
      timeout: 5_000,
    });
    await browser.keys("Escape");
    expect(
      await invoke<string[]>("grants_list", { subject: { type: "thread", threadId: thread.id } }),
    ).toEqual([github]);
    expect(
      await invoke<string[]>("grants_list", { subject: { type: "thread", threadId: other.id } }),
    ).toEqual([]);

    const ok = await sendAndWait(thread.id, 'mcp tandem__call {"tool":"github__get_me","args":{}}', 60_000);
    expect(ok.join(" ")).toMatch(/The tool said:[\s\S]*octo-shop/);
  });
});
