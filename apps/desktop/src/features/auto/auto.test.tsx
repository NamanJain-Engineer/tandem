import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { axe } from "vitest-axe";
import type { AutoRunView } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useAuto } from "../../stores/auto";
import { useSettings } from "../../stores/settings";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { AutoChip } from "./AutoChip";
import { AutoDialog } from "./AutoDialog";

const run = (over: Partial<AutoRunView> = {}): AutoRunView => ({
  runId: "au_1",
  paneId: "pn_1",
  mode: "finish",
  goal: null,
  ceiling: 20,
  turns: 3,
  state: "running",
  paused: null,
  endReason: null,
  detail: null,
  startedAt: 0,
  endedAt: null,
  ...over,
});

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

beforeEach(() => {
  resetStores();
  useAuto.setState({ byPane: {}, dialogFor: null });
});

function dialog(handlers: Parameters<typeof fakeBackend>[0] = {}) {
  calls = fakeBackend({ auto_start: (a) => run({ mode: a?.mode as AutoRunView["mode"], turns: 0 }), ...handlers });
  render(
    <UiProvider>
      <AutoDialog />
    </UiProvider>,
  );
  act(() => useAuto.getState().openDialog("pn_1"));
}

describe("Auto (F-AU-01)", () => {
  it("F-AU-01 first use asks consent before anything is sent, and records it", async () => {
    dialog();
    expect(screen.getByText("Finish")).toBeTruthy();
    expect(screen.getByText(/Up to 20 turns/)).toBeTruthy();
    fireEvent.click(screen.getByTestId("auto-start"));
    await screen.findByTestId("auto-consent");
    expect(screen.getByText(/last ~200 lines of this terminal \(secrets redacted\)/)).toBeTruthy();
    expect(named("auto_start")).toHaveLength(0);
    fireEvent.click(screen.getByRole("checkbox", { name: "Don't ask again" }));
    fireEvent.click(screen.getByTestId("auto-consent-continue"));
    await waitFor(() => expect(named("auto_start")).toHaveLength(1));
    expect(named("settings_set").map((c) => c.args)).toEqual([
      { key: "auto_consent", value: true },
      { key: "auto_dont_ask", value: true },
    ]);
    expect(named("auto_start")[0]?.args).toEqual({ paneId: "pn_1", mode: "finish", goal: null, ceiling: null });
    await waitFor(() => expect(useAuto.getState().dialogFor).toBeNull());
    expect(useAuto.getState().byPane.pn_1?.mode).toBe("finish");
  });

  it("with consent and Don't ask again, Start starts at once; Custom sends its goal and turns", async () => {
    dialog();
    act(() => useSettings.setState({ values: { auto_consent: true, auto_dont_ask: true }, loaded: true }));
    fireEvent.click(screen.getByRole("radio", { name: /Custom/ }));
    const start = screen.getByTestId("auto-start") as HTMLButtonElement;
    expect(start.disabled).toBe(true);
    fireEvent.change(screen.getByTestId("auto-goal"), { target: { value: "make CI green" } });
    fireEvent.change(screen.getByTestId("auto-ceiling"), { target: { value: "5" } });
    fireEvent.click(start);
    await waitFor(() => expect(named("auto_start")).toHaveLength(1));
    expect(named("auto_start")[0]?.args).toEqual({ paneId: "pn_1", mode: "custom", goal: "make CI green", ceiling: 5 });
    expect(screen.queryByTestId("auto-consent")).toBeNull();
  });

  it("a refusal shows its message and keeps the dialog open", async () => {
    dialog({
      auto_start: () => {
        throw { code: "forbidden_plan", message: "Auto needs Vibezeo Pro." };
      },
    });
    act(() => useSettings.setState({ values: { auto_consent: true, auto_dont_ask: true }, loaded: true }));
    fireEvent.click(screen.getByTestId("auto-start"));
    expect((await screen.findByTestId("auto-error")).textContent).toBe("Auto needs Vibezeo Pro.");
    expect(useAuto.getState().dialogFor).toBe("pn_1");
  });

  it("the chip shows ⚡ Finish · 3/20 while live, its end with the reason, and dismisses", async () => {
    calls = fakeBackend();
    act(() => useAuto.getState().put(run()));
    const { container } = render(
      <UiProvider>
        <AutoChip paneId="pn_1" />
      </UiProvider>,
    );
    expect(screen.getByTestId("auto-chip").textContent).toContain("⚡ Finish · 3/20");
    expect(await axe(container)).toHaveNoViolations();
    act(() => useAuto.getState().put(run({ state: "paused", paused: "approval_pending" })));
    expect(screen.getByTestId("auto-chip").textContent).toContain("Paused");
    act(() => useAuto.getState().put(run({ state: "ended", turns: 20, endReason: "turn_ceiling" })));
    expect(screen.getByTestId("auto-chip").textContent).toBe("Stopped · Reached the turn limit.");
    act(() => useAuto.getState().put(run({ state: "ended", endReason: "done", detail: "All 12 tests pass." })));
    expect(screen.getByTestId("auto-chip").textContent).toBe("Done · All 12 tests pass.");
    fireEvent.click(screen.getByTestId("auto-dismiss"));
    await waitFor(() => expect(named("auto_dismiss")).toHaveLength(1));
    expect(screen.queryByTestId("auto-chip")).toBeNull();
  });
});
