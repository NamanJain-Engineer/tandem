import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { BestOfDetail, BestOfLane, BestOfRun, EngineView, FileDiff, GitPanel, WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useEngines } from "../../stores/engines";
import { useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { BestOf, branchPreview, useBestOf } from "./BestOf";

const WS = { id: "ws_1", root: "/work/shop", name: "shop", tabs: [{ id: "tab_1" }], branch: "main" } as unknown as WorkspaceSummary;

const engine = (id: string, label: string): EngineView =>
  ({
    spec: { id, label, logo: label.slice(0, 2) },
    install: { path: `/usr/bin/${id}`, version: "1.0.0", source: "shell" },
    chatCapable: true,
    unsupportedReason: null,
    extraArgs: [],
  }) as unknown as EngineView;

function lane(seat: number, over: Partial<BestOfLane> = {}): BestOfLane {
  return {
    seat,
    laneId: `ws_l${seat}`,
    branch: `tandem/fix-login-${seat + 1}`,
    engineId: seat === 1 ? "codex" : "claude",
    accountId: null,
    paneId: `pn_${seat}`,
    error: null,
    check: "none",
    checkExit: null,
    checkOutput: null,
    ...over,
  };
}

function run(lanes: BestOfLane[], over: Partial<BestOfRun> = {}): BestOfRun {
  return { id: "bo_1", workspaceId: "ws_1", title: "Fix login", tabId: null, base: "abc123", checkCommand: "pnpm test", state: "running", createdAt: 0, lanes, ...over };
}

function file(path: string, additions: number): FileDiff {
  return { path, change: "modified", additions, deletions: 1, binary: false, patch: `diff --git a/${path} b/${path}\n--- a/${path}\n+++ b/${path}\n@@ -1 +1 @@\n-old\n+new\n`, truncated: false } as FileDiff;
}

const count = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).length;

type Opts = { git?: GitPanel | null; detail?: () => BestOfDetail; keep?: () => unknown };

function setup({ git = { folder: "/work/shop", status: { staged: [], unstaged: [] } } as unknown as GitPanel, detail, keep }: Opts = {}) {
  const lanes = [lane(0), lane(1), lane(2)];
  const calls = fakeBackend({
    workspace_list: () => [WS],
    git_status: () => git,
    layout_get: () => null,
    bestof_start: () => run(lanes),
    bestof_list: () => [run(lanes)],
    bestof_get:
      detail ??
      (() => ({
        run: run(lanes),
        diffs: [
          { seat: 0, files: [file("src/login.ts", 4)], error: null },
          { seat: 1, files: [file("src/login.ts", 2), file("src/auth.ts", 7)], error: null },
          { seat: 2, files: [], error: null },
        ],
      })),
    bestof_check: () => null,
    bestof_keep: keep ?? (() => ({ merged: true, conflicts: [] })),
    bestof_discard: () => null,
  });
  useWorkspaces.setState({ list: [WS], activeId: "ws_1", activeTab: { ws_1: "tab_1" } });
  useEngines.setState({ list: [engine("claude", "Claude Code"), engine("codex", "Codex")], loaded: true });
  render(
    <UiProvider>
      <BestOf />
    </UiProvider>,
  );
  return calls;
}

describe("Best of N (UI-143, UI-144)", () => {
  beforeEach(() => resetStores());

  it("F-EX-04.1 the sheet starts the same prompt for each seat and opens Compare", async () => {
    const calls = setup();
    act(() => useBestOf.getState().openSheet("ws_1"));
    const sheet = await screen.findByTestId("bestof-sheet");
    await waitFor(() => expect(count(calls, "git_status")).toBe(1));
    expect(within(sheet).getAllByTestId("bestof-seat").length).toBe(2);
    fireEvent.click(within(sheet).getByTestId("bestof-add"));
    expect(within(sheet).getAllByTestId("bestof-seat").length).toBe(3);
    expect(within(sheet).queryByTestId("bestof-add")).toBeNull();
    fireEvent.change(within(sheet).getByTestId("bestof-prompt"), { target: { value: "Fix login redirect" } });
    fireEvent.change(within(sheet).getByTestId("bestof-check"), { target: { value: " pnpm test " } });
    expect(within(sheet).getByTestId("bestof-summary").textContent).toBe(
      "This opens 3 agents in 3 worktrees (tandem/fix-login-redirect-1 … -3), one per agent.",
    );
    fireEvent.keyDown(within(sheet).getByTestId("bestof-prompt"), { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(count(calls, "bestof_start")).toBe(1));
    expect(calls.find((c) => c.cmd === "bestof_start")?.args).toEqual({
      workspaceId: "ws_1",
      prompt: "Fix login redirect",
      seats: [
        { engineId: "claude", accountId: null },
        { engineId: "codex", accountId: null },
        { engineId: "claude", accountId: null },
      ],
      checkCommand: "pnpm test",
    });
    await waitFor(() => expect(useBestOf.getState().compare).toBe("bo_1"));
    expect(branchPreview("x", 1)).toBe("tandem/x");
  });

  it("F-EX-04.1 seats get an engine once the engines load after the sheet opens", async () => {
    const calls = setup();
    useEngines.setState({ list: [], loaded: false });
    act(() => useBestOf.getState().openSheet("ws_1"));
    const sheet = await screen.findByTestId("bestof-sheet");
    // The sheet asks for the engines when nothing has loaded them yet.
    await waitFor(() => expect(count(calls, "engines_list")).toBe(1));
    act(() => useEngines.setState({ list: [engine("claude", "Claude Code")], loaded: true }));
    fireEvent.change(within(sheet).getByTestId("bestof-prompt"), { target: { value: "Fix it" } });
    await waitFor(() => expect((within(sheet).getByTestId("bestof-start") as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(within(sheet).getByTestId("bestof-start"));
    await waitFor(() => expect(count(calls, "bestof_start")).toBe(1));
    expect(calls.find((c) => c.cmd === "bestof_start")?.args?.seats).toEqual([
      { engineId: "claude", accountId: null },
      { engineId: "claude", accountId: null },
    ]);
  });

  it("F-EX-04.1 a workspace with uncommitted changes gets the note", async () => {
    setup({ git: { folder: "/work/shop", status: { staged: [], unstaged: [{ path: "a.txt", change: "modified" }] } } as unknown as GitPanel });
    act(() => useBestOf.getState().openSheet("ws_1"));
    expect(await screen.findByText("Worktrees start from the last commit; your uncommitted changes aren't in them.")).toBeTruthy();
  });

  it("F-EX-04.5 a folder that isn't a git repository says why and can't start", async () => {
    const calls = setup({ git: null });
    act(() => useBestOf.getState().openSheet("ws_1"));
    expect(await screen.findByText("Best of N needs a git repository")).toBeTruthy();
    expect(screen.queryByTestId("bestof-start")).toBeNull();
    expect(count(calls, "bestof_start")).toBe(0);
  });

  it("F-EX-04.2 Compare shows one column per lane with its own changes", async () => {
    setup();
    act(() => useBestOf.getState().openCompare("bo_1"));
    const cols = await screen.findAllByTestId("bestof-column");
    expect(cols.map((c) => `${c.dataset.branch}|${c.dataset.files}`)).toEqual([
      "tandem/fix-login-1|1",
      "tandem/fix-login-2|2",
      "tandem/fix-login-3|0",
    ]);
    expect(cols[1]?.getAttribute("aria-label")).toBe("Codex");
    expect(cols[1]?.textContent).toContain("2 files +9 −2");
    expect(cols[2]?.textContent).toContain("No changes yet.");
  });

  it("F-EX-04.3 Run check runs it, and each column shows its exit code and output", async () => {
    let lanes = [lane(0), lane(1), lane(2)];
    const calls = setup({ detail: () => ({ run: run(lanes), diffs: [] }) });
    act(() => useBestOf.getState().openCompare("bo_1"));
    fireEvent.click(await screen.findByTestId("bestof-run-check"));
    await waitFor(() => expect(count(calls, "bestof_check")).toBe(1));
    expect(calls.find((c) => c.cmd === "bestof_check")?.args).toEqual({ runId: "bo_1" });
    lanes = [
      lane(0, { check: "passed", checkExit: 0, checkOutput: "ok" }),
      lane(1, { check: "failed", checkExit: 1, checkOutput: "1 failing" }),
      lane(2, { check: "running" }),
    ];
    fireEvent.click(screen.getByTestId("bestof-run-check"));
    await waitFor(() => expect(screen.getAllByTestId("bestof-column").map((c) => c.dataset.check)).toEqual(["passed", "failed", "running"]));
    const cols = screen.getAllByTestId("bestof-column");
    expect(cols[0]?.textContent).toContain("Passed");
    expect(cols[1]?.textContent).toContain("Failed · exit 1");
    expect(cols[2]?.textContent).toContain("Running…");
    fireEvent.click(within(cols[1]!).getByRole("button", { name: "Output" }));
    expect((await screen.findByTestId("bestof-output")).textContent).toBe("1 failing");
  });

  it("F-EX-04.4 Keep this one keeps that seat; Cancel in the native confirm changes nothing and says nothing", async () => {
    let answer: "cancel" | "keep" = "cancel";
    const calls = setup({
      keep: () => {
        if (answer === "cancel") throw { code: "cancelled", message: "Kept every worktree", stage: null, hint: null, retryable: false };
        return { merged: true, conflicts: [] };
      },
    });
    act(() => useBestOf.getState().openCompare("bo_1"));
    const cols = await screen.findAllByTestId("bestof-column");
    fireEvent.click(within(cols[1]!).getByTestId("bestof-keep"));
    await waitFor(() => expect(count(calls, "bestof_keep")).toBe(1));
    expect(calls.find((c) => c.cmd === "bestof_keep")?.args).toEqual({ runId: "bo_1", seat: 1 });
    await waitFor(() => expect((within(screen.getAllByTestId("bestof-column")[1]!).getByTestId("bestof-keep") as HTMLButtonElement).disabled).toBe(false));
    expect(screen.queryByText("Kept every worktree")).toBeNull();
    answer = "keep";
    fireEvent.click(within(screen.getAllByTestId("bestof-column")[1]!).getByTestId("bestof-keep"));
    await waitFor(() => expect(count(calls, "bestof_keep")).toBe(2));
  });

  it("F-EX-04.4 a dirty base's refusal is shown", async () => {
    setup({
      keep: () => {
        throw { code: "conflict", message: "Commit or stash your changes in shop first.", stage: "bestof", hint: null, retryable: false };
      },
    });
    act(() => useBestOf.getState().openCompare("bo_1"));
    fireEvent.click(within((await screen.findAllByTestId("bestof-column"))[0]!).getByTestId("bestof-keep"));
    expect(await screen.findByText("Commit or stash your changes in shop first.")).toBeTruthy();
  });

  it("F-EX-04 a merged run shows the result and no actions", async () => {
    setup({ detail: () => ({ run: run([lane(0), lane(1)], { state: "merged" }), diffs: [] }) });
    act(() => useBestOf.getState().openCompare("bo_1"));
    const view = await screen.findByTestId("bestof-compare");
    expect(view.dataset.state).toBe("merged");
    expect(view.textContent).toContain("Merged into shop.");
    expect(screen.queryByTestId("bestof-keep")).toBeNull();
    expect(screen.queryByTestId("bestof-discard")).toBeNull();
  });
});
