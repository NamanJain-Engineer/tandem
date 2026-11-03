import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { CloseRequest } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { CloseDialog } from "./CloseDialog";

const TWO: CloseRequest = {
  id: 1,
  agents: [
    { key: "pane:pn_1", title: "Fix the login flow" },
    { key: "thread:th_1", title: null },
  ],
  quit: false,
};

async function setup() {
  const calls = fakeBackend({ app_close_decide: () => null, app_close_seen: () => null });
  render(
    <UiProvider>
      <CloseDialog />
    </UiProvider>,
  );
  // Its listener subscribes after the first render.
  await act(async () => undefined);
  return calls;
}

beforeEach(resetStores);
afterEach(() => setInvokeBackend(null));

describe("closing with agents running (F-SH-14)", () => {
  it("F-SH-14.1 asks to keep them running, names them, and Keep running keeps them", async () => {
    const calls = await setup();
    act(() => emitLocal("app:close-requested", TWO));
    expect(await screen.findByText("Keep 2 agents running in the background?")).toBeTruthy();
    expect(screen.getByText("Fix the login flow")).toBeTruthy();
    fireEvent.click(screen.getByTestId("close-keep"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "app_close_decide")?.args).toEqual({ keep: true, remember: false }),
    );
    await waitFor(() => expect(screen.queryByTestId("close-agents")).toBeNull());
  });

  it("F-SH-14.4 F-SH-14.5 Stop them and quit, with Don't ask again, stops them and saves the choice", async () => {
    const calls = await setup();
    act(() => emitLocal("app:close-requested", { ...TWO, agents: TWO.agents.slice(0, 1) }));
    expect(await screen.findByText("Keep 1 agent running in the background?")).toBeTruthy();
    fireEvent.click(screen.getByLabelText("Don't ask again"));
    fireEvent.click(screen.getByTestId("close-stop"));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "app_close_decide")?.args).toEqual({ keep: false, remember: true }),
    );
  });

  it("F-SH-14 tells Vibezeo it showed each question, so the close doesn't go ahead without it", async () => {
    const calls = await setup();
    act(() => emitLocal("app:close-requested", TWO));
    expect(await screen.findByTestId("close-agents")).toBeTruthy();
    await waitFor(() =>
      expect(calls.filter((c) => c.cmd === "app_close_seen").map((c) => c.args)).toEqual([{ id: 1 }]),
    );
    // Closing again while it's open asks again, and that's shown too.
    act(() => emitLocal("app:close-requested", { ...TWO, id: 2 }));
    await waitFor(() =>
      expect(calls.filter((c) => c.cmd === "app_close_seen").map((c) => c.args)).toEqual([
        { id: 1 },
        { id: 2 },
      ]),
    );
  });

  it("F-SH-14 dismissing the dialog keeps the window open and decides nothing", async () => {
    const calls = await setup();
    act(() => emitLocal("app:close-requested", TWO));
    expect(await screen.findByTestId("close-agents")).toBeTruthy();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    await waitFor(() => expect(screen.queryByTestId("close-agents")).toBeNull());
    expect(calls.some((c) => c.cmd === "app_close_decide")).toBe(false);
  });
});
