import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AuthState, EntitlementView, ThreadDetail } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { TitleBar } from "../features/shell/TitleBar";
import { setInvokeBackend } from "../ipc/invoke";
import { useSession } from "../stores/session";
import { useThreads } from "../stores/threads";
import { useUi } from "../stores/ui";
import { useWorkspaces } from "../stores/workspaces";
import { resetStores } from "../test/fakeBackend";
import { DEMO_MESSAGE_LIMIT, DEMO_WAITING, DemoCore } from "./backend";
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
    const view = mount(
      <>
        <div data-testid="worktree-row" data-branch={DEMO_WAITING.branch} />
        <button type="button" role="radio" aria-checked="false" value="thread" />
        <DemoCoach core={c} />
      </>,
    );
    const hint = () => screen.getAllByTestId("coachmark");
    expect(hint()).toHaveLength(1);
    expect(hint()[0]?.textContent).toContain(`${DEMO_WAITING.engine} is waiting on you. Open it to see why.`);
    expect(hint()[0]?.textContent).toContain("1 of 3");
    // Under the waiting worktree's row, in the rail's free space: over no
    // row and none of the panes' text.
    expect(hint()[0]?.dataset.side).toBe("bottom");
    // Folded, its repository's row carries the hint, beside the rail.
    view.rerender(
      <UiProvider theme="dark">
        <div data-testid="workspace-row" data-root={DEMO_WAITING.repoRoot} />
        <button type="button" role="radio" aria-checked="false" value="thread" />
        <DemoCoach core={c} />
      </UiProvider>,
    );
    await act(() => new Promise((r) => requestAnimationFrame(() => r(undefined))));
    expect(hint()[0]?.textContent).toContain(`${DEMO_WAITING.engine} is waiting on you.`);
    expect(hint()[0]?.dataset.side).toBe("right");
    act(() => useWorkspaces.setState({ activeId: DEMO_WAITING.workspaceId }));
    // In Code mode the task waits one switch away.
    expect(hint()).toHaveLength(1);
    expect(hint()[0]?.textContent).toContain("Your first task is ready in Thread mode.");
    expect(hint()[0]?.textContent).toContain("2 of 3");
    // Thread mode: the same hint, on the composer.
    view.rerender(
      <UiProvider theme="dark">
        <div data-testid="composer" />
        <button type="button" data-testid="changes-toggle" />
        <DemoCoach core={c} />
      </UiProvider>,
    );
    // The coach looks again on the next frame after the page changes.
    await act(() => new Promise((r) => requestAnimationFrame(() => r(undefined))));
    expect(hint()[0]?.textContent).toContain("Press Enter to send this task.");
    expect(hint()[0]?.textContent).toContain("2 of 3");
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
    expect(hint()[0]?.textContent).toContain("3 of 3");
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
