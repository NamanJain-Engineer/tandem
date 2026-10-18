import { afterEach, describe, expect, it } from "vitest";
import type { PaneRecord, TabLayout } from "@tandem/protocol";
import { setInvokeBackend } from "../ipc/invoke";
import { resetStores } from "../test/fakeBackend";
import { useLayout } from "./layout";

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

function pane(id: string, tabId: string, workspaceId: string): PaneRecord {
  return {
    id,
    tabId,
    workspaceId,
    kind: "terminal",
    cwd: "/tmp",
    engineId: null,
    accountId: null,
    humanName: null,
    title: null,
    restoreState: "live",
    browserUrl: null,
    createdAt: 1,
  };
}

afterEach(() => setInvokeBackend(null));

describe("layout store (F-CD-02)", () => {
  it("F-CD-02 workspaces that load at the same time keep each other's panes", async () => {
    resetStores();
    // ws_a answers slowly; ws_b loads and finishes in the middle of it.
    setInvokeBackend(async (cmd, args) => {
      const a = args as Record<string, string> | undefined;
      if (cmd === "layout_get") {
        const ws = a?.workspaceId ?? "";
        const tab = { id: `tb_${ws}`, workspaceId: ws, name: ws, sortKey: "a0", createdAt: 1 };
        const layout: TabLayout = { tab, tree: { type: "leaf", paneId: `pn_${ws}` }, activePaneId: `pn_${ws}`, maximizedPaneId: null };
        return [layout];
      }
      if (cmd === "pane_list") {
        const ws = (a?.tabId ?? "").replace(/^tb_/, "");
        await wait(ws === "ws_a" ? 30 : 1);
        return [pane(`pn_${ws}`, a?.tabId ?? "", ws)];
      }
      return null;
    });
    await Promise.all([useLayout.getState().load("ws_a"), useLayout.getState().load("ws_b")]);
    const tabs = useLayout.getState().tabs;
    expect(Object.keys(tabs[`tb_ws_a`]?.panes ?? {})).toEqual(["pn_ws_a"]);
    expect(Object.keys(tabs[`tb_ws_b`]?.panes ?? {})).toEqual(["pn_ws_b"]);
  });

  it("F-WT-09 an empty tab expecting another flow's pane starts a shell only if none came (D-863)", async () => {
    resetStores();
    const created: string[] = [];
    setInvokeBackend(async (cmd, args) => {
      const tabId = String((args as Record<string, unknown> | undefined)?.tabId ?? "");
      if (cmd !== "pane_create") return null;
      created.push(tabId);
      return pane(`pn_shell_${tabId}`, tabId, "ws_a");
    });
    const layout = useLayout.getState();
    // A new worktree's Preview pane is placed while the tab waits: no shell.
    let placed: () => void = () => undefined;
    layout.expect("tb_a", new Promise<void>((r) => (placed = r)));
    const first = layout.ensurePane("tb_a");
    await wait(5);
    layout.insert("tb_a", pane("pn_preview", "tb_a", "ws_a"), null, "right");
    placed();
    await first;
    expect(created).toEqual([]);
    expect(Object.keys(useLayout.getState().tabs.tb_a?.panes ?? {})).toEqual(["pn_preview"]);
    // The flow placed nothing (its review opened instead), or failed: the shell comes after.
    let failed: (e: Error) => void = () => undefined;
    layout.expect("tb_b", new Promise<void>((_, j) => (failed = j)));
    const second = layout.ensurePane("tb_b");
    await wait(5);
    expect(created).toEqual([]);
    failed(new Error("no preview"));
    await second;
    expect(created).toEqual(["tb_b"]);
    expect(Object.keys(useLayout.getState().tabs.tb_b?.panes ?? {})).toEqual(["pn_shell_tb_b"]);
  });
});
