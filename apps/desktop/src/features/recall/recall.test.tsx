import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { HistoryEntry, PaneRecord, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useLayout } from "../../stores/layout";
import { useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { terminalHandles, type TerminalHandle } from "../code/TerminalView";
import { RecallPicker, canRun, duration, useRecall } from "./RecallPicker";

const WS = { id: "ws_1", root: "/work/shop", name: "shop", tabs: [{ id: "tab_1" }], branch: "main" } as unknown as WorkspaceSummary;

function entry(id: number, command: string, over: Partial<HistoryEntry> = {}): HistoryEntry {
  return {
    id,
    command,
    cwd: "/work/shop",
    exitCode: 0,
    durationMs: 1200,
    paneId: "pn_1",
    paneName: "shell",
    workspaceId: "ws_1",
    lastRunAt: Date.now() - 60_000,
    runs: 1,
    secret: false,
    ...over,
  };
}

const ROWS = [entry(2, "pnpm test", { runs: 3 }), entry(1, "false", { exitCode: 1 }), entry(3, "export GITHUB_TOKEN=[REDACTED]", { secret: true })];

function pane(kind: string): PaneRecord {
  return { id: "pn_1", tabId: "tab_1", workspaceId: "ws_1", kind, cwd: null, engineId: kind === "agent" ? "claude" : null, accountId: null, humanName: "shell", title: null, restoreState: "none", createdAt: 0, browserUrl: null } as PaneRecord;
}

let pasted: string[] = [];

function setup(kind = "terminal", rows: HistoryEntry[] = ROWS) {
  const calls = fakeBackend({
    workspace_list: () => [WS],
    history_search: (a) => rows.filter((r) => r.command.includes(String(a?.query ?? ""))),
    history_run: () => null,
    history_delete: () => null,
    settings_set: () => null,
  });
  useUi.setState({ mode: "code" });
  useWorkspaces.setState({ list: [WS], activeId: "ws_1", activeTab: { ws_1: "tab_1" } });
  useLayout.setState({ tabs: { tab_1: { tree: null, active: "pn_1", maximized: null, panes: { pn_1: pane(kind) }, keepEmpty: false } } });
  pasted = [];
  terminalHandles.set("pn_1", { paste: (t: string) => pasted.push(t), focus: () => undefined } as unknown as TerminalHandle);
  render(
    <UiProvider>
      <RecallPicker />
    </UiProvider>,
  );
  act(() => useRecall.getState().show());
  return calls;
}

const count = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).length;
const input = () => screen.getByPlaceholderText("Search commands you've run");

describe("Recall palette (UI-141)", () => {
  beforeEach(() => resetStores());
  afterEach(() => terminalHandles.clear());

  it("F-EX-02 lists commands newest first with exit, runs and folder; searches", async () => {
    const calls = setup();
    const rows = await screen.findAllByTestId("recall-row");
    expect(rows.map((r) => r.dataset.command)).toEqual(ROWS.map((r) => r.command));
    expect(rows[0]?.textContent).toContain("×3");
    expect(rows[1]?.textContent).toContain("✗ 1");
    expect(calls.find((c) => c.cmd === "history_search")?.args).toMatchObject({ workspaceId: "ws_1" });
    fireEvent.change(input(), { target: { value: "pnpm" } });
    await waitFor(() => expect(screen.getAllByTestId("recall-row").map((r) => r.dataset.command)).toEqual(["pnpm test"]));
    fireEvent.click(screen.getByRole("radio", { name: "Everywhere" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "history_search").at(-1)?.args).toMatchObject({ workspaceId: null }));
  });

  it("F-EX-02.4 Enter inserts into the pane with no newline and runs nothing", async () => {
    const calls = setup();
    await screen.findAllByTestId("recall-row");
    fireEvent.keyDown(input(), { key: "Enter" });
    await waitFor(() => expect(pasted).toEqual(["pnpm test"]));
    expect(pasted[0]?.endsWith("\r")).toBe(false);
    expect(count(calls, "history_run")).toBe(0);
    expect(useRecall.getState().open).toBe(false);
  });

  it("F-EX-02.4 Mod+Enter runs it once in a shell pane", async () => {
    const calls = setup();
    await screen.findAllByTestId("recall-row");
    fireEvent.keyDown(input(), { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(count(calls, "history_run")).toBe(1));
    expect(calls.find((c) => c.cmd === "history_run")?.args).toEqual({ id: 2, paneId: "pn_1" });
    expect(pasted).toEqual([]);
  });

  it("F-EX-02.2 a command with a hidden secret is never run; an agent pane never runs one", async () => {
    expect(canRun(ROWS[2]!, { kind: "pane", paneId: "p", workspaceId: "w", agent: false, name: null })).toBe(false);
    expect(canRun(ROWS[0]!, { kind: "pane", paneId: "p", workspaceId: "w", agent: true, name: null })).toBe(false);
    expect(canRun(ROWS[0]!, { kind: "composer", threadId: "t" })).toBe(false);
    const calls = setup("terminal", [ROWS[2]!]);
    await screen.findAllByTestId("recall-row");
    fireEvent.keyDown(input(), { key: "Enter", metaKey: true });
    expect(await screen.findByText("This command has a hidden secret. Insert it and fill in the secret.")).toBeTruthy();
    expect(count(calls, "history_run")).toBe(0);
  });

  it("F-EX-02.5 Mod+Backspace deletes; off and empty states", async () => {
    const calls = setup();
    await screen.findAllByTestId("recall-row");
    fireEvent.keyDown(input(), { key: "Backspace", ctrlKey: true });
    await waitFor(() => expect(count(calls, "history_delete")).toBe(1));
    expect(calls.find((c) => c.cmd === "history_delete")?.args).toEqual({ id: 2 });
    act(() => useSettings.setState({ values: { command_history_enabled: false } }));
    expect(await screen.findByText("Command recall is off")).toBeTruthy();
  });

  it("F-EX-02 empty history says where commands come from", async () => {
    setup("terminal", []);
    expect(await screen.findByText("Commands you run in Vibezeo terminals show up here.")).toBeTruthy();
  });

  it("F-EX-02 durations read plainly", () => {
    expect(duration(null)).toBe("");
    expect(duration(120)).toBe("120ms");
    expect(duration(14_000)).toBe("14s");
    expect(duration(252_000)).toBe("4:12");
  });
});
