import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AuthState, EntitlementView } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { TitleBar } from "../features/shell/TitleBar";
import { setInvokeBackend } from "../ipc/invoke";
import { useSession } from "../stores/session";
import { resetStores } from "../test/fakeBackend";
import { DEMO_MESSAGE_LIMIT, DemoCore } from "./backend";
import { DemoBar } from "./DemoBar";
import { DemoHints, HINT_KEY } from "./DemoHints";

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
  it("demo first steps: the hint says how to send the ready message and what to try; Dismiss keeps only that flag", () => {
    mount(<DemoHints />);
    const hint = screen.getByTestId("demo-hints");
    expect(hint.textContent).toContain("Press Enter to send it");
    for (const step of ["Answer the approval", "Open Changes", "Ctrl+2", "help", "Ctrl+K"]) expect(hint.textContent).toContain(step);
    fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(screen.queryByTestId("demo-hints")).toBeNull();
    expect(Object.keys(localStorage)).toEqual([HINT_KEY]);
    // A later visit doesn't show it again.
    mount(<DemoHints />);
    expect(screen.queryByTestId("demo-hints")).toBeNull();
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
