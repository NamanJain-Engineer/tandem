import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type {
  EngineView,
  HandoffBrief,
  HandoffResult,
  PaneRecord,
  Tab,
  WorkspaceSummary,
} from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useEngines } from "../../stores/engines";
import { defaultTarget, useHandoff, type HandoffSource } from "../../stores/handoff";
import { useLayout } from "../../stores/layout";
import { useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { HandoffDialog } from "./HandoffDialog";

const engine = (id: string, label: string, installed = true): EngineView =>
  ({
    spec: { id, label, logo: label[0] },
    install: installed ? { version: "1.0.0", path: `/bin/${id}` } : null,
    chatCapable: false,
    unsupportedReason: null,
    extraArgs: [],
  }) as unknown as EngineView;

function ws(id: string, opts: { branch?: string | null; parentId?: string | null } = {}): WorkspaceSummary {
  const tab: Tab = { id: `tb_${id}`, workspaceId: id, name: "Tab 1", sortKey: "a0", createdAt: 0 };
  return {
    id,
    kind: "local",
    hostId: "local",
    root: `/code/${id}`,
    wslDistro: null,
    name: id,
    parentId: opts.parentId ?? null,
    laneBranch: opts.parentId ? "tandem/fix-auth" : null,
    laneBase: opts.parentId ? "main" : null,
    sortKey: "a0",
    pinned: false,
    collapsed: false,
    sessionsCollapsed: false,
    createdAt: 0,
    missing: false,
    branch: opts.branch ?? null,
    liveCount: 0,
    tabs: [tab],
  };
}

const pane = (id: string, workspaceId: string, engineId: string | null): PaneRecord => ({
  id,
  tabId: `tb_${workspaceId}`,
  workspaceId,
  kind: engineId ? "agent" : "terminal",
  cwd: null,
  engineId,
  accountId: null,
  humanName: null,
  title: null,
  restoreState: "none",
  createdAt: 0,
});

const BRIEF = "## Goal\nFix auth\n## What's done\nx\n## Current state\ny\n## Next steps\nz";
let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

function open(
  workspace: WorkspaceSummary,
  source: HandoffSource,
  handlers: Parameters<typeof fakeBackend>[0] = {},
): void {
  calls = fakeBackend({ workspace_list: () => [workspace], layout_get: () => [], ...handlers });
  useWorkspaces.setState({
    list: [workspace],
    activeId: workspace.id,
    activeTab: { [workspace.id]: `tb_${workspace.id}` },
  });
  useEngines.setState({
    list: [engine("codex", "Codex"), engine("claude", "Claude Code"), engine("gemini", "Gemini CLI", false)],
    loaded: true,
  });
  useHandoff.setState({ source: null, notes: {} });
  render(
    <UiProvider>
      <HandoffDialog />
    </UiProvider>,
  );
  useHandoff.getState().open(source);
}

const codexPane: HandoffSource = { paneId: "pn_src", workspaceId: "shop", kind: "agent", engineId: "codex" };

beforeEach(() => {
  resetStores();
  useLayout.setState({ tabs: {}, loaded: {}, boxes: {}, dragging: false, picker: null });
});
afterEach(() => setInvokeBackend(null));

describe("handoff (F-CD-16, UI-13)", () => {
  it("F-CD-16 the target defaults to another installed engine, Claude Code first", () => {
    const list = [
      engine("codex", "Codex"),
      engine("claude", "Claude Code"),
      engine("gemini", "Gemini", false),
    ];
    expect(defaultTarget(list, "codex")).toBe("claude");
    expect(defaultTarget(list, "claude")).toBe("codex");
    expect(defaultTarget([engine("claude", "Claude Code")], "claude")).toBe("claude");
    expect(defaultTarget([], null)).toBeNull();
  });

  it("F-CD-16.1 the summary is written on open, can be edited, and Start sends it with the target and place", async () => {
    let resolve: (b: HandoffBrief) => void = () => undefined;
    const brief = new Promise<HandoffBrief>((r) => (resolve = r));
    const result: HandoffResult = {
      pane: { ...pane("pn_new", "shop", "claude"), humanName: "Builder" },
      workspaceId: "shop",
      tabId: "tb_shop",
      laneBranch: null,
      sourceClosed: false,
    };
    open(ws("shop", { branch: "main" }), codexPane, {
      handoff_brief: () => brief,
      handoff_start: () => result,
    });
    expect(await screen.findByText("Hand off to another agent")).toBeTruthy();
    expect(
      screen.getByText("Vibezeo will ask Codex to write a handoff summary, then start Claude Code with it."),
    ).toBeTruthy();
    expect(screen.getByRole("status", { name: "Writing summary…" })).toBeTruthy();
    expect((screen.getByTestId("handoff-start") as HTMLButtonElement).disabled).toBe(true);
    expect(named("handoff_brief")[0]?.args).toMatchObject({ paneId: "pn_src", engineId: "claude" });

    resolve({ text: BRIEF, source: "source" });
    const summary = (await screen.findByTestId("handoff-summary")) as HTMLTextAreaElement;
    expect(summary.value).toBe(BRIEF);
    fireEvent.change(summary, { target: { value: `${BRIEF}\nAlso run the linter.` } });
    fireEvent.click(screen.getByRole("checkbox", { name: "Close the original pane after handoff" }));
    fireEvent.click(screen.getByTestId("handoff-start"));
    await waitFor(() => expect(named("handoff_start")).toHaveLength(1));
    expect(named("handoff_start")[0]?.args).toEqual({
      paneId: "pn_src",
      engineId: "claude",
      accountId: null,
      brief: `${BRIEF}\nAlso run the linter.`,
      place: "same",
      closeSource: true,
    });
    await waitFor(() => expect(useHandoff.getState().source).toBeNull());
    expect(useHandoff.getState().notes.pn_src).toBe("Handed off to Builder (Claude Code)");
    useHandoff.getState().note("pn_src", null);
    expect(useHandoff.getState().notes.pn_src).toBeUndefined();
  });

  it("UI-13 when no summary can be written, it says so and the empty box can be filled in", async () => {
    open(ws("shop", { branch: "main" }), codexPane, {
      handoff_brief: () => {
        throw { code: "not_found", message: "No pane", stage: null, hint: null, retryable: false };
      },
    });
    expect(await screen.findByText("Couldn't generate a summary. You can write one yourself.")).toBeTruthy();
    const summary = screen.getByTestId("handoff-summary") as HTMLTextAreaElement;
    expect(summary.value).toBe("");
    expect(summary.disabled).toBe(false);
    expect((screen.getByTestId("handoff-start") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(summary, { target: { value: "## Goal\nKeep going" } });
    expect((screen.getByTestId("handoff-start") as HTMLButtonElement).disabled).toBe(false);
  });

  it("F-CD-16.3 a fallback summary says it holds the session's last lines; a terminal source reads differently", async () => {
    open(
      ws("notes"),
      { paneId: "pn_sh", workspaceId: "notes", kind: "terminal", engineId: null },
      { handoff_brief: () => ({ text: "## Goal\nContinue", source: "fallback" }) },
    );
    expect(await screen.findByTestId("handoff-fallback")).toBeTruthy();
    expect(
      screen.getByText(
        "Vibezeo will write a handoff summary from this terminal's output, then start Claude Code with it.",
      ),
    ).toBeTruthy();
  });
});
