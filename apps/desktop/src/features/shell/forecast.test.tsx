import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Account, AccountSessions, EngineView, PaneRecord, UsageView, WindowForecast } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useAccounts } from "../../stores/accounts";
import { useEngines } from "../../stores/engines";
import { useLayout } from "../../stores/layout";
import { useUsage } from "../../stores/usage";
import { useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { seatUsageHint } from "../code/LaunchSheet";
import { durationText, forecastLine, hasRoom, isTight, whoRows } from "./forecast";
import { UsageChip } from "./UsageChip";

const MIN = 60_000;
const NOW = new Date(2026, 9, 15, 14, 10).getTime();

const fc = (patch: Partial<WindowForecast>): WindowForecast => ({
  kind: "5h",
  state: "on_track",
  runsOutAt: null,
  beforeResetMs: null,
  ratePerHour: null,
  basisMinutes: 30,
  ...patch,
});

const view = (accountId: string, pcts: number[], forecasts: WindowForecast[] = []): UsageView => ({
  accountId,
  engineId: "codex",
  windows: pcts.map((usedPct, i) => ({ kind: i === 0 ? "5h" : "week", usedPct, resetsAt: NOW + 120 * MIN })),
  source: "codex_app_server",
  estimated: false,
  reason: null,
  fetchedAt: NOW,
  forecasts,
});

const account = (id: string, label: string, isDefault: boolean): Account => ({
  id,
  engineId: "codex",
  label,
  homePath: `/p/${id}`,
  isDefault,
  planLabel: null,
  createdAt: 0,
});

beforeEach(() => resetStores());
afterEach(() => {
  vi.useRealTimers();
  setInvokeBackend(null);
});

describe("usage forecast (F-AC-08)", () => {
  it("F-AC-08.1 a window that runs out before its reset says when, and by how much", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(NOW);
    const line = forecastLine(
      fc({ state: "runs_out", runsOutAt: NOW + 60 * MIN, beforeResetMs: 30 * MIN, basisMinutes: 30 }),
      NOW,
    );
    expect(line).toMatch(/^Runs out at 3:10\s?PM, 30 min before it resets · based on the last 30 min$/);
    expect(durationText(90 * MIN)).toBe("1 h 30 min");
    expect(durationText(51 * 60 * MIN)).toBe("2 d 3 h");
  });

  it("F-AC-08.2 too little data and on-track windows say so", () => {
    expect(forecastLine(fc({ state: "not_enough_data" }))).toBe("Not enough data yet");
    expect(forecastLine(fc({ state: "on_track", basisMinutes: 45 }))).toBe("On track · based on the last 45 min");
    expect(forecastLine(undefined)).toBeNull();
  });

  it("F-AC-08.4 who's using it names the pane, else another session", () => {
    const row: AccountSessions = {
      accountId: "acct_1",
      engineId: "claude",
      readable: true,
      sessions: [
        { sessionId: "ses-a", paneId: "pn_1", tokens: 3_000, share: 0.75 },
        { sessionId: "ses-b", paneId: null, tokens: 1_000, share: 0.25 },
      ],
    };
    const rows = whoRows(row, (id) => (id === "pn_1" ? "fix-auth" : null));
    expect(rows.map((r) => r.label)).toEqual(["fix-auth · 75%", "Another session · 25%"]);
  });

  it("F-AC-08.6 a seat near its limit says so and names an account with more room; nothing switches", () => {
    const accounts = [account("acct_work", "Work", true), account("acct_home", "Home", false)];
    const usage = { acct_work: view("acct_work", [85]), acct_home: view("acct_home", [20, 10]) };
    expect(seatUsageHint("codex", null, "ws_1", accounts, [], usage)).toEqual({
      tight: "Work is at 85%",
      room: "Home has more room",
    });
    // Forecast to run out counts as tight even below 80%.
    const short = { acct_work: view("acct_work", [60], [fc({ state: "runs_out" })]) };
    expect(isTight(short.acct_work)).toEqual({ pct: 60 });
    expect(seatUsageHint("codex", null, "ws_1", accounts, [], short)?.room).toBeNull();
    // A seat's own account comes first.
    expect(seatUsageHint("codex", "acct_home", "ws_1", accounts, [], usage)).toBeNull();
    // Comfortable accounts get no hint.
    expect(seatUsageHint("codex", null, "ws_1", accounts, [], { acct_work: view("acct_work", [40]) })).toBeNull();
    expect(hasRoom(view("x", [49, 10]))).toBe(true);
    expect(hasRoom(view("x", [49, 60]))).toBe(false);
    expect(hasRoom(undefined)).toBe(false);
  });

  it("F-AC-08 the popover shows each window's forecast and who's using the account", async () => {
    const calls = fakeBackend({
      usage_get: () => [],
      usage_refresh: () => [],
      usage_sessions: () => [
        {
          accountId: "acct_work",
          engineId: "codex",
          readable: true,
          sessions: [{ sessionId: "ses-a", paneId: "pn_1", tokens: 10, share: 1 }],
        },
      ],
    });
    useEngines.setState({
      list: [{ spec: { id: "codex", label: "Codex", logo: "C" }, install: null } as unknown as EngineView],
      loaded: true,
    });
    useAccounts.setState({ list: [account("acct_work", "Work", true)], overrides: [], loaded: true });
    useWorkspaces.setState({ list: [], activeId: "ws_1", activeTab: { ws_1: "tb_1" } });
    const pane = {
      id: "pn_1",
      tabId: "tb_1",
      workspaceId: "ws_1",
      kind: "agent",
      engineId: "codex",
      accountId: "acct_work",
      humanName: "fix-auth",
    } as PaneRecord;
    useLayout.setState({
      tabs: {
        tb_1: { tree: { type: "leaf", paneId: "pn_1" }, active: "pn_1", maximized: null, panes: { pn_1: pane }, keepEmpty: false },
      },
    });
    useUsage.getState().set([
      view("acct_work", [60], [fc({ state: "runs_out", runsOutAt: NOW + 60 * MIN, beforeResetMs: 30 * MIN })]),
    ]);
    render(
      <UiProvider>
        <UsageChip />
      </UiProvider>,
    );
    expect(screen.getByTestId("usage-chip").title).toMatch(/Runs out at/);
    fireEvent.click(screen.getByTestId("usage-chip"));
    const line = await screen.findByTestId("usage-forecast");
    expect(line.dataset.state).toBe("runs_out");
    await waitFor(() => expect(screen.getByTestId("usage-who-row").textContent).toBe("fix-auth · 100%"));
    expect(calls.some((c) => c.cmd === "usage_sessions")).toBe(true);
  });

  it("F-AC-08 who's using it has loading, error, unreadable and empty states", async () => {
    fakeBackend({ usage_sessions: () => Promise.reject(new Error("nope")) });
    await useUsage.getState().loadSessions();
    expect(useUsage.getState().sessionsState).toBe("error");
    fakeBackend({ usage_sessions: () => [{ accountId: "a", engineId: "gemini", readable: false, sessions: [] }] });
    await useUsage.getState().loadSessions();
    expect(useUsage.getState().sessionsState).toBe("ready");
    expect(useUsage.getState().sessions.a?.readable).toBe(false);
  });
});
