import { describe, expect, it, beforeEach } from "vitest";
import type { PaneRecord } from "@tandem/protocol";
import { useLayout } from "../../stores/layout";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { launchInTerminal } from "./launchInTerminal";

const PANE = {
  id: "pn_scribe",
  tabId: "tb_1",
  workspaceId: "ws_1",
  kind: "agent",
  cwd: "/code/shop",
  engineId: "claude",
  accountId: null,
  humanName: "Scribe",
  title: null,
  restoreState: "none",
  createdAt: 1,
  browserUrl: null,
} as PaneRecord;

describe("F-AG-13 Launch in terminal", () => {
  beforeEach(() => resetStores());

  it("F-AG-13.1 opens the teammate's pane in Code mode, focused, in the chosen place", async () => {
    const calls = fakeBackend({
      teammate_launch_in_terminal: () => PANE,
      workspace_list: () => [
        { id: "ws_1", root: "/code/shop", name: "shop", tabs: [{ id: "tb_1", workspaceId: "ws_1", name: "Tab 1" }] },
      ],
      layout_get: () => null,
      pane_list: () => [PANE],
    });
    useUi.getState().setMode("agent");
    const r = await launchInTerminal("tm_scribe", "/code/shop");
    expect(r.ok).toBe(true);
    expect(calls.find((c) => c.cmd === "teammate_launch_in_terminal")?.args).toEqual({
      id: "tm_scribe",
      placePath: "/code/shop",
      tabId: null,
    });
    expect(useUi.getState().mode).toBe("code");
    expect(useWorkspaces.getState().activeId).toBe("ws_1");
    expect(useLayout.getState().tabs.tb_1?.active).toBe("pn_scribe");
  });

  it("F-AG-13 an engine that isn't installed says so", async () => {
    fakeBackend({
      teammate_launch_in_terminal: () => {
        throw { code: "engine_missing", message: "Claude Code isn't installed" };
      },
    });
    useUi.getState().setMode("agent");
    const r = await launchInTerminal("tm_scribe", "/code/shop");
    expect(r.ok).toBe(false);
    expect(!r.ok && r.error.message).toBe("Claude Code isn't installed");
    expect(useUi.getState().mode).toBe("agent");
  });
});
