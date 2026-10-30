import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AuthState, EntitlementView, ThreadDetail } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { TitleBar } from "../features/shell/TitleBar";
import { setInvokeBackend } from "../ipc/invoke";
import { useSession } from "../stores/session";
import { paneKey, useStatus } from "../stores/status";
import { useThreads } from "../stores/threads";
import { useUi } from "../stores/ui";
import { useWorkspaces } from "../stores/workspaces";
import { resetStores } from "../test/fakeBackend";
import { DEMO_MESSAGE_LIMIT, DEMO_WAITING, DemoCore } from "./backend";
import { worktreeIn } from "./story";
import { DemoBar } from "./DemoBar";
import { DemoCoach, HINT_KEY } from "./DemoCoach";

let core: DemoCore | null = null;
beforeEach(() => {
  resetStores();
  localStorage.clear();
});
afterEach(() => {
  core?.dispose();
  setInvokeBackend(null);
});

function mount(ui: React.ReactNode) {
  return render(<UiProvider theme="dark">{ui}</UiProvider>);
}

describe("the interactive demo's page furniture", () => {
  it("DESIGN-5.2.2 demo coach: one hint at a time beside what it describes, each gone once the visitor does it", async () => {
    core = new DemoCore(0);
    const c = core;
    // The coach stays mounted as the page changes around it, a scene at a
    // time: the app swaps what it shows, so the coach looks again.
    const scene = (name: string, nodes: React.ReactNode) => (
      <UiProvider theme="dark">
        <DemoCoach core={c} />
        <div key={name}>{nodes}</div>
      </UiProvider>
    );
    const radio = <button type="button" role="radio" aria-checked="false" value="thread" />;
    const bea = <div data-testid="pane" data-pane-id={DEMO_WAITING.paneId} />;
    const view = render(
      scene(
        "rail",
        <>
          <div data-testid="worktree-row" data-branch={DEMO_WAITING.branch} />
          {radio}
        </>,
      ),
    );
    // The coach looks again on the next frame after the page changes.
    const show = async (name: string, nodes: React.ReactNode) => {
      view.rerender(scene(name, nodes));
      await act(() => new Promise((r) => requestAnimationFrame(() => r(undefined))));
    };
    const hint = () => screen.getAllByTestId("coachmark");
    expect(hint()).toHaveLength(1);
    // The agent by its first name, as 0.12.0 names every agent (F-CD-12).
    expect(DEMO_WAITING.name).toBe(worktreeIn("waiting").agentName);
    expect(hint()[0]?.textContent).toContain(`${DEMO_WAITING.name} is waiting on you. Open it to see why.`);
    expect(hint()[0]?.textContent).toContain("1 of 4");
    // Under the waiting worktree's row, in the rail's free space: over no
    // row and none of the panes' text.
    expect(hint()[0]?.dataset.side).toBe("bottom");
    // Folded, its repository's row carries the hint, beside the rail.
    await show(
      "folded",
      <>
        <div data-testid="workspace-row" data-root={DEMO_WAITING.repoRoot} />
        {radio}
      </>,
    );
    expect(hint()[0]?.textContent).toContain(`${DEMO_WAITING.name} is waiting on you.`);
    expect(hint()[0]?.dataset.side).toBe("right");
    // Opened, she asks in her pane. Its question is the engine's, with none
    // of its choices (D-2633), so the coach names the keys in the demo's own
    // words, from her pane's foot, clear of the question at its top.
    const asking = (status: "waiting" | "working") =>
      act(() =>
        useStatus.setState({
          records: {
            [paneKey(DEMO_WAITING.paneId)]: {
              key: paneKey(DEMO_WAITING.paneId),
              status,
              reason: null,
              since: 1,
              ts: 1,
              tool: null,
              title: null,
              preview: null,
            },
          },
        }),
      );
    asking("waiting");
    act(() => useWorkspaces.setState({ activeId: DEMO_WAITING.workspaceId }));
    await show(
      "bea",
      <>
        {bea}
        {radio}
      </>,
    );
    expect(hint()).toHaveLength(1);
    expect(hint()[0]?.textContent).toContain(
      `Press Enter to let ${DEMO_WAITING.name} run it, or Esc to say no.`,
    );
    expect(hint()[0]?.textContent).toContain("2 of 4");
    expect(hint()[0]?.dataset.side).toBe("bottom");
    // Left unanswered, the task's hint shows once her question isn't on
    // screen: Thread mode keeps her workspace active. One thing at a time.
    await show("thread", <div data-testid="composer" />);
    expect(useWorkspaces.getState().activeId).toBe(DEMO_WAITING.workspaceId);
    expect(hint()[0]?.textContent).toContain("Press Enter to send this task.");
    expect(hint()[0]?.textContent).toContain("3 of 4");
    await show(
      "bea-again",
      <>
        {bea}
        {radio}
      </>,
    );
    expect(hint()[0]?.textContent).toContain("Press Enter to let");
    // Answered: in Code mode the task waits one switch away.
    asking("working");
    expect(hint()).toHaveLength(1);
    expect(hint()[0]?.textContent).toContain("Your first task is ready in Thread mode.");
    expect(hint()[0]?.textContent).toContain("3 of 4");
    // Thread mode: the same hint, on the composer.
    await show(
      "thread-again",
      <>
        <div data-testid="composer" />
        <button type="button" data-testid="changes-toggle" />
      </>,
    );
    expect(hint()[0]?.textContent).toContain("Press Enter to send this task.");
    expect(hint()[0]?.textContent).toContain("3 of 4");
    act(() => {
      useThreads.setState({ list: [{ ...(c.handle("thread_get", { threadId: c.firstThread }) as ThreadDetail).thread, state: "working" }] });
      c.handle("thread_send", { threadId: c.firstThread, text: "hi", attachments: [] });
    });
    // Changes waits until the agent has edited something.
    expect(screen.queryByTestId("coachmark")).toBeNull();
    act(() => useThreads.setState((st) => ({ list: st.list.map((x) => ({ ...x, state: "waiting" as const })) })));
    expect(hint()[0]?.textContent).toContain("Open Changes to see what it edited.");
    // In the header band beside the button, off the transcript.
    expect(hint()[0]?.dataset.side).toBe("left");
    expect(hint()[0]?.textContent).toContain("4 of 4");
    act(() => useThreads.getState().toggleChanges(c.firstThread));
    expect(screen.queryByTestId("coachmark")).toBeNull();
    expect(localStorage.getItem(HINT_KEY)).toBe("1");
  });

  it("DESIGN-5.2.1 the demo opens in Code mode; Thread mode first shows the thread with its message ready", () => {
    core = new DemoCore(0);
    const c = core;
    setInvokeBackend((cmd, args) => Promise.resolve(c.handle(cmd, args as Record<string, unknown>)));
    act(() => useThreads.setState({ loaded: true, selectedId: null }));
    mount(<DemoBar core={c} firstThread={c.firstThread} />);
    expect(useThreads.getState().selectedId).toBeNull();
    act(() => useUi.setState({ mode: "thread" }));
    expect(useThreads.getState().selectedId).toBe(c.firstThread);
  });

  it("demo AC5 past the limit the bar says Demo limit reached with Download for Linux; Reset the demo starts over", () => {
    core = new DemoCore(0);
    const c = core;
    const onReset = vi.fn();
    mount(<DemoBar core={c} firstThread={c.firstThread} onReset={onReset} />);
    expect(screen.queryByText("Demo limit reached")).toBeNull();
    const id = c.firstThread;
    act(() => {
      for (let i = 0; i < DEMO_MESSAGE_LIMIT; i++) {
        c.handle("thread_send", { threadId: id, text: `m${i}`, attachments: [] });
        c.handle("thread_stop", { threadId: id });
      }
    });
    expect(screen.getByText("Demo limit reached")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Download for Linux" }).getAttribute("href")).toBe("/download/");
    fireEvent.click(screen.getByRole("button", { name: "Reset the demo" }));
    expect(onReset).toHaveBeenCalled();
  });

  it("demo embedded on the home page the bar leaves out the link back to the site, and says the agents are simulated", () => {
    core = new DemoCore(0);
    mount(<DemoBar core={core} firstThread={core.firstThread} embedded />);
    expect(screen.getByTestId("demo-bar").textContent).toMatch(/simulated/i);
    expect(screen.queryByRole("link", { name: "vibezeo.com" })).toBeNull();
  });

  it("F-WB-01 demo the title bar shows no credits and no account, as the early-access download doesn't", async () => {
    core = new DemoCore(0);
    const c = core;
    setInvokeBackend(async (cmd, args) => c.handle(cmd, args ?? {}));
    useSession.setState({
      auth: c.handle("auth_state", {}) as AuthState,
      entitlement: c.handle("entitlement_get", {}) as EntitlementView,
    });
    mount(<TitleBar />);
    expect(screen.queryByRole("button", { name: /credits/i })).toBeNull();
    fireEvent.keyDown(screen.getByRole("button", { name: "Account" }), { key: "Enter" });
    expect(await screen.findByText("No account")).toBeTruthy();
    expect(screen.queryByText(/@/)).toBeNull();
  });
});
