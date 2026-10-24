import { $, browser } from "@wdio/globals";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import {
  addFolder,
  crash,
  engineSpawns,
  fakeLog,
  freshUser,
  invoke,
  paneIds,
  pausedCards,
  waitSignedIn,
  relaunch,
  sendToPane,
  signInDesktop,
  stopOrphansAutomatically,
  waitForApp,
  waitForPaneText,
  waitForSessionStarts,
  waitForShell,
} from "../support/app";

type Node =
  { type: "leaf"; paneId: string } | { type: "split"; dir: "row" | "col"; ratio: number; a: Node; b: Node };
type Layout = { tab: { id: string }; tree: Node | null };

function balanced(nodes: Node[], dir: "row" | "col"): Node {
  if (nodes.length === 1) return nodes[0] as Node;
  const half = Math.ceil(nodes.length / 2);
  return {
    type: "split",
    dir,
    ratio: half / nodes.length,
    a: balanced(nodes.slice(0, half), dir),
    b: balanced(nodes.slice(half), dir),
  };
}
const leaves = (ids: string[]): Node[] => ids.map((paneId) => ({ type: "leaf", paneId }));

// E2E-M5-1 Crash restore (F-SH-07 AC1–2).
describe("E2E-M5-1 crash restore", () => {
  let email = "";
  let wsId = "";
  let shells: string[] = [];
  let agents: string[] = [];
  let dirs: string[] = [];

  after(async () => {
    await stopOrphansAutomatically(false).catch(() => undefined);
  });

  it("F-SH-07.1 after kill -9 with 4 shells and 4 agents: the same layout, 4 live shells at their cwds, 4 paused cards", async function () {
    this.timeout(300_000);
    ({ email } = await freshUser("m5-1"));

    await stopOrphansAutomatically(true);
    const root = `/tmp/tandem-e2e-m5-1-${Date.now()}`;
    await addFolder(root);
    await browser.waitUntil(async () => (await paneIds()).length === 1, { timeout: 20_000 });
    const [ws] = await invoke<Array<{ id: string; tabs: Array<{ id: string }> }>>("workspace_list");
    wsId = ws?.id ?? "";
    const tabId = ws?.tabs[0]?.id ?? "";
    shells = [(await paneIds())[0] ?? ""];
    for (let i = 1; i < 4; i++)
      shells.push(
        (await invoke<{ id: string }>("pane_create", { tabId, kind: "terminal", cwd: null, engineId: null }))
          .id,
      );
    const logFrom = fakeLog().length;
    agents = [];
    for (let i = 0; i < 4; i++)
      agents.push(
        (await invoke<{ id: string }>("pane_create", { tabId, kind: "agent", cwd: null, engineId: "claude" }))
          .id,
      );
    const tree = balanced([balanced(leaves(shells), "row"), balanced(leaves(agents), "row")], "col");
    await invoke("layout_save", { tabId, tree, activePaneId: shells[0], maximizedPaneId: null });
    await browser.refresh();
    await waitForApp();
    await browser.waitUntil(async () => (await paneIds()).length === 8, { timeout: 30_000 });
    // Each shell in its own folder (OSC 7 stores the cwd).
    dirs = shells.map((_, i) => join(root, `dir-${i}`));
    for (const d of dirs) mkdirSync(d, { recursive: true });
    for (const [i, s] of shells.entries()) {
      await waitForShell(s);
      await sendToPane(s, `cd ${dirs[i]}\n`);
    }
    await browser.waitUntil(
      async () => {
        const rows = await invoke<Array<{ id: string; cwd: string | null }>>("pane_list", { tabId });
        return shells.every((id, i) => rows.find((r) => r.id === id)?.cwd === dirs[i]);
      },
      { timeout: 15_000, timeoutMsg: "the shells never reported their cwds" },
    );
    for (const a of agents) await waitForPaneText(a, "Fake Claude Code");
    await waitForSessionStarts(logFrom, 4);
    const before = (await invoke<Layout[]>("layout_get", { workspaceId: wsId })).map((l) => l.tree);
    const enginesBefore = (await engineSpawns()).length;

    crash();
    await relaunch();
    // The in-memory test keychain doesn't survive a relaunch: sign in again.
    await signInDesktop(email);
    await browser.waitUntil(async () => (await paneIds()).length === 8, {
      timeout: 30_000,
      timeoutMsg: `panes: ${await paneIds()}`,
    });
    expect((await invoke<Layout[]>("layout_get", { workspaceId: wsId })).map((l) => l.tree)).toEqual(before);
    await browser.waitUntil(async () => (await pausedCards()).length === 4, { timeout: 15_000 });
    expect((await pausedCards()).map((c) => c.paneId).sort()).toEqual([...agents].sort());
    expect((await pausedCards()).every((c) => c.state === "paused")).toBe(true);
    for (const [i, s] of shells.entries()) {
      await waitForShell(s);
      await sendToPane(s, "echo CWD=$(pwd)\n");
      await waitForPaneText(s, `CWD=${dirs[i]}`);
    }
    // Nothing an agent runs started by itself.
    expect((await engineSpawns()).length).toBe(enginesBefore);
  });

  it("F-SH-07.2 Resume on one card spawns exactly one process, in the saved provider home, with --resume <id>", async function () {
    this.timeout(60_000);
    await waitSignedIn();
    const before = await engineSpawns();
    const savedHome = before.at(-1)?.providerHome ?? null;
    expect(savedHome).not.toBeNull();
    const agent = agents[0] ?? "";
    await $(`[data-testid="pane"][data-pane-id="${agent}"] [data-testid="restore-resume"]`).click();
    await waitForPaneText(agent, "Resumed session fake-");
    const fresh = (await engineSpawns()).slice(before.length);
    expect(fresh).toHaveLength(1);
    expect(fresh[0]?.providerHome).toBe(savedHome);
    // The wrapper quotes each argument: '--resume' 'fake-123'.
    const id = /--resume (fake-\d+)/.exec((fresh[0]?.argv.join(" ") ?? "").replace(/'/g, ""))?.[1];
    expect(id).toBeDefined();
    await waitForPaneText(agent, `Resumed session ${id}`);
    expect((await pausedCards()).map((c) => c.paneId).sort()).toEqual(agents.slice(1).sort());
  });
});
