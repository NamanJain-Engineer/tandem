import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Account, EngineView, HandoffResult, LimitView, PaneRecord } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useAccounts } from "../../stores/accounts";
import { useEngines } from "../../stores/engines";
import { until, useLimits } from "../../stores/limits";
import { useLayout } from "../../stores/layout";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { LimitBanner } from "./LimitCard";

const engine = (id: string, label: string): EngineView =>
  ({
    spec: { id, label, logo: label[0] },
    install: { version: "1.0.0", path: `/bin/${id}` },
    chatCapable: false,
    unsupportedReason: null,
    extraArgs: [],
  }) as unknown as EngineView;

const account = (id: string, engineId: string, label: string, isDefault = false): Account => ({
  id,
  engineId,
  label,
  homePath: `/p/${id}`,
  isDefault,
  planLabel: null,
  createdAt: 0,
});

const pane = {
  id: "pn_1",
  tabId: "tb_1",
  workspaceId: "ws_1",
  kind: "agent",
  engineId: "claude",
  accountId: "acct_work",
} as PaneRecord;
const RESET = Date.now() + 108 * 60_000;
const LIMIT: LimitView = {
  paneId: "pn_1",
  accountId: "acct_work",
  message: "Claude usage limit reached",
  resetsAt: RESET,
  resumeAt: null,
};

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

function setup(limit: LimitView = LIMIT, handlers: Parameters<typeof fakeBackend>[0] = {}) {
  calls = fakeBackend({ layout_get: () => [], ...handlers });
  useAccounts.setState({
    list: [account("acct_work", "claude", "Work", true), account("acct_personal", "claude", "Personal")],
    overrides: [],
    loaded: true,
    failed: false,
    signIn: {},
  });
  useEngines.setState({ list: [engine("claude", "Claude Code"), engine("codex", "Codex")], loaded: true });
  useLimits.setState({ byPane: { pn_1: limit } });
  render(
    <UiProvider>
      <LimitBanner pane={pane} />
    </UiProvider>,
  );
}

beforeEach(() => {
  resetStores();
  useLayout.setState({ tabs: {}, loaded: {}, boxes: {}, dragging: false, picker: null });
});
afterEach(() => setInvokeBackend(null));

describe("limit card (F-AC-05, UI-36)", () => {
  it("F-AC-05 the countdown is hours and minutes, never negative", () => {
    expect(until(1_000 + 108 * 60_000, 1_000)).toEqual({ h: 1, m: 48 });
    expect(until(0, 60_000)).toEqual({ h: 0, m: 0 });
  });

  it("F-AC-05.2 the banner names the account and reset; Resume at reset schedules it, and it can be cancelled", async () => {
    const scheduled = { ...LIMIT, resumeAt: RESET };
    setup(LIMIT, { limit_resume_at_reset: () => scheduled, limit_cancel_resume: () => LIMIT });
    const banner = screen.getByTestId("limit-banner");
    expect(banner.textContent).toMatch(/^Work reached its limit · resets .+ \(in 1h 4[78]m\)/);
    fireEvent.click(screen.getByTestId("limit-resume-at-reset"));
    await waitFor(() => expect(named("limit_resume_at_reset")[0]?.args).toEqual({ paneId: "pn_1" }));
    // The core's limit:changed shows the paused state.
    useLimits.getState().set("pn_1", scheduled);
    await waitFor(() => expect(screen.getByTestId("limit-banner").dataset.scheduled).toBe("true"));
    expect(screen.getByTestId("limit-banner").textContent).toMatch(/^Paused · resumes /);
    fireEvent.click(screen.getByTestId("limit-cancel-resume"));
    await waitFor(() => expect(named("limit_cancel_resume")).toHaveLength(1));
  });

  it("F-AC-05.1 no other account is used until the summary is confirmed", async () => {
    const moved = {
      pane: { ...pane, id: "pn_2", accountId: "acct_personal" },
      workspaceId: "ws_1",
      tabId: "tb_1",
      laneBranch: null,
      sourceClosed: false,
    } as HandoffResult;
    setup(LIMIT, { limit_continue: () => moved });
    fireEvent.click(screen.getByTestId("limit-choose"));
    const dialog = await screen.findByTestId("limit-dialog");
    expect(
      within(dialog).getByText(
        "Wait for it to reset, or continue on another account. Your conversation and files are kept.",
      ),
    ).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("radio", { name: /Continue on another account/ }));
    fireEvent.click(screen.getByTestId("limit-go"));
    expect(await screen.findByText("Continue with a summary?")).toBeTruthy();
    expect(
      screen.getByText(
        "Recent messages and completed action names carry over; detailed tool history doesn't.",
      ),
    ).toBeTruthy();
    expect(named("limit_continue")).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() =>
      expect(named("limit_continue")[0]?.args).toEqual({
        paneId: "pn_1",
        engineId: "claude",
        accountId: "acct_personal",
      }),
    );
  });

  it("UI-36 without a reset time, Resume at reset is off and the dialog says why", async () => {
    setup({ ...LIMIT, resetsAt: null });
    expect(screen.getByTestId("limit-banner").textContent).toMatch(/^Work reached its limit/);
    expect((screen.getByTestId("limit-resume-at-reset") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByTestId("limit-choose"));
    expect(await screen.findByText("The engine didn't say when this limit resets.")).toBeTruthy();
  });
});
