import { $, browser } from "@wdio/globals";
import { addFolder, clickAllow, freshOffline, invoke, paneIds } from "../support/app";
import { sendAndWait } from "../support/agents";
import { connectGithub, mockCalls, mockReset, openPlugins, openTabNamed } from "../support/plugins";

type Draft = { id: string; displayNo: number; state: string; originId: string; tool: string };

// E2E-M12-5 Held draft (F-PL-08, F-PL-06 unattended): a routine's run calls
// a write tool; nothing reaches the upstream, the agent is told "Held for
// review (draft id D-n)" and the draft waits in Held actions. Approve & run
// sends exactly one request, and a second run of the same draft is refused.
describe("E2E-M12-5 held draft", () => {
  let draft: Draft | undefined;

  before(async function () {
    this.timeout(90_000);
    await mockReset();
    await freshOffline();
    await addFolder(`/tmp/tandem-e2e-m12-5-${Date.now()}`);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
  });

  it("F-PL-08.1 a routine's write is held, not sent", async function () {
    this.timeout(90_000);
    const github = await connectGithub();
    const [ws] = await invoke<Array<{ id: string }>>("workspace_list");
    const run = await invoke<{ id: string }>("test_unattended_run", {
      workspaceId: ws?.id,
      routineId: "rt_nightly",
      runId: "run_1",
    });
    await invoke("grants_set", {
      subject: { type: "thread", threadId: run.id },
      connectionId: github,
      granted: true,
    });
    const reply = await sendAndWait(
      run.id,
      'mcp tandem__call {"tool":"github__create_issue","args":{"owner":"octo-shop","repo":"web","title":"Nightly bump"}}',
      60_000,
    );
    expect(reply.join(" ")).toMatch(/Held for review \(draft id D-\d+\)/);
    expect(await mockCalls("create_issue")).toBe(0);
    const drafts = await invoke<Draft[]>("held_drafts_list", { state: "held", originId: "rt_nightly" });
    expect(drafts).toHaveLength(1);
    draft = drafts[0];
    expect(draft?.tool).toBe("create_issue");
  });

  it("F-PL-08.2 Approve & run sends exactly one request", async function () {
    this.timeout(60_000);
    await openPlugins();
    await openTabNamed('[data-testid="plugins-page"]', "Held actions");
    const row = $(`[data-testid="held-${draft?.displayNo}"]`);
    await row.waitForDisplayed({ timeout: 10_000 });
    expect(await row.getText()).toContain("Held · from routine rt_nightly");
    await clickAllow(`[data-testid="held-run-${draft?.displayNo}"]`);
    await row.waitForExist({ reverse: true, timeout: 20_000 });
    expect(await mockCalls("create_issue")).toBe(1);
    await expect(invoke("held_draft_run", { draftId: draft?.id, editedArgs: null })).rejects.toThrow(
      /conflict/,
    );
    expect(await mockCalls("create_issue")).toBe(1);
    expect(await invoke<Draft[]>("held_drafts_list", { state: "held", originId: null })).toEqual([]);
  });
});
