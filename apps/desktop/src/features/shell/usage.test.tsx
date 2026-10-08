import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Account, EngineView, PaneRecord, StatusRecord, UsageView } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { useAccounts } from "../../stores/accounts";
import { useEngines } from "../../stores/engines";
import { useLayout } from "../../stores/layout";
import { useStatus } from "../../stores/status";
import { followUsage, fullest, useUsage } from "../../stores/usage";
import { useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { UsageChip, usageChip } from "./UsageChip";

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

const RESET = new Date(2026, 9, 8, 16, 10).getTime();
const view = (accountId: string, engineId: string, pcts: number[], reason: string | null = null): UsageView => ({
  accountId,
  engineId,
  windows: pcts.map((usedPct, i) => ({ kind: i === 0 ? "5h" : "week", usedPct, resetsAt: RESET + i })),
  source: pcts.length > 0 ? "codex_app_server" : "unavailable",
  estimated: false,
  reason,
  fetchedAt: 0,
});

const pane = (id: string, accountId: string | null, kind = "agent"): PaneRecord =>
  ({ id, tabId: "tb_1", workspaceId: "ws_1", kind, engineId: kind === "agent" ? "codex" : null, accountId }) as PaneRecord;

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

/** A focused pane in the active tab, and two accounts. */
function setup(focused: PaneRecord | null, handlers: Parameters<typeof fakeBackend>[0] = {}) {
  calls = fakeBackend({ usage_refresh: () => [], usage_get: () => [], ...handlers });
  useEngines.setState({ list: [engine("claude", "Claude Code"), engine("codex", "Codex")], loaded: true });
  useAccounts.setState({
    list: [account("acct_codex", "codex", "Work", true), account("acct_claude", "claude", "Personal", true)],
    overrides: [],
    loaded: true,
    failed: false,
    signIn: {},
  });
  useWorkspaces.setState({ list: [], activeId: "ws_1", activeTab: { ws_1: "tb_1" } });
  useLayout.setState({
    tabs: {
      tb_1: {
        tree: focused ? { type: "leaf", paneId: focused.id } : null,
        active: focused?.id ?? null,
        maximized: null,
        panes: focused ? { [focused.id]: focused } : {},
        keepEmpty: false,
      },
    },
  });
}

const renderChip = () =>
  render(
    <UiProvider>
      <UsageChip />
    </UiProvider>,
  );

beforeEach(() => {
  resetStores();
  useLayout.setState({ tabs: {}, loaded: {}, boxes: {}, dragging: false, picker: null });
});
afterEach(() => {
  vi.useRealTimers();
  setInvokeBackend(null);
});

describe("usage chip (F-AC-04)", () => {
  it("F-AC-04.1 windows at 40% and 85% show the 85% window in warning", () => {
    setup(pane("pn_1", "acct_codex"));
    useUsage.getState().set([view("acct_codex", "codex", [40, 85])]);
    renderChip();
    const chip = screen.getByTestId("usage-chip");
    expect(chip.dataset.tone).toBe("warning");
    expect(chip.textContent).toMatch(/^Codex · 85% · resets 4:10/);
    expect(chip.title).toBe("Weekly, the closest to its limit. Click for every account.");
    expect(fullest(view("a", "codex", [40, 85]).windows)?.kind).toBe("week");
  });

  it("F-AC-04 the chip turns danger at 95% and stays neutral below 80%", () => {
    expect(usageChip(view("a", "codex", [95, 10]), "Codex").tone).toBe("danger");
    expect(usageChip(view("a", "codex", [79]), "Codex").tone).toBe("muted");
    expect(usageChip(view("a", "codex", [80]), "Codex").tone).toBe("warning");
  });

  it("F-AC-04.2 without usage data the chip says Usage unavailable and the tooltip explains the source", () => {
    const reason = "Claude Code doesn't report usage to other apps.";
    setup(pane("pn_1", "acct_claude"));
    useUsage.getState().set([view("acct_claude", "claude", [], reason)]);
    renderChip();
    const chip = screen.getByTestId("usage-chip");
    expect(chip.textContent).toBe("Usage unavailable");
    expect(chip.title).toBe(reason);
    expect(chip.dataset.tone).toBe("muted");
    // Not read yet: still unavailable, and the tooltip says it's checking.
    expect(usageChip(undefined, "Codex")).toMatchObject({ text: "Usage unavailable", tooltip: "Checking usage…" });
  });

  it("F-AC-04 with nothing focused the chip follows the default engine's default account", () => {
    setup(pane("pn_1", null, "terminal"));
    useUsage.getState().set([view("acct_claude", "claude", [62])]);
    renderChip();
    expect(screen.getByTestId("usage-chip").textContent).toMatch(/^Claude Code · 62%/);
  });

  it("F-AC-04 clicking the chip lists every account's windows", async () => {
    setup(pane("pn_1", "acct_codex"));
    useUsage
      .getState()
      .set([view("acct_codex", "codex", [40, 85]), view("acct_claude", "claude", [], "Not reported.")]);
    renderChip();
    fireEvent.click(screen.getByTestId("usage-chip"));
    const pop = await screen.findByTestId("usage-popover");
    expect(pop.textContent).toContain("Work · Codex");
    expect(pop.textContent).toContain("Personal · Claude Code");
    expect(pop.textContent).toContain("Not reported.");
    expect(pop.querySelectorAll("[role=meter]")).toHaveLength(2);
    fireEvent.click(screen.getByTestId("usage-refresh"));
    await waitFor(() => expect(named("usage_refresh")[0]?.args).toEqual({ accountId: null }));
  });

  it("F-AC-04 an account without a reading is asked for once, and the answer shows", async () => {
    setup(pane("pn_1", "acct_codex"), { usage_get: () => [view("acct_codex", "codex", [30])] });
    const { rerender } = renderChip();
    expect(screen.getByTestId("usage-chip").textContent).toBe("Usage unavailable");
    await waitFor(() => expect(screen.getByTestId("usage-chip").textContent).toMatch(/^Codex · 30%/));
    rerender(
      <UiProvider>
        <UsageChip />
      </UiProvider>,
    );
    expect(named("usage_get")).toEqual([{ cmd: "usage_get", args: { accountId: "acct_codex" } }]);
  });

  it("F-AC-04 usage loads at start, follows usage:changed, refreshes every 5 minutes and after a turn", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] });
    setup(pane("pn_1", "acct_codex"), { usage_get: () => [view("acct_codex", "codex", [10])] });
    const stop = followUsage();
    await vi.waitFor(() => expect(useUsage.getState().byAccount.acct_codex?.windows[0]?.usedPct).toBe(10));
    act(() => emitLocal("usage:changed", view("acct_codex", "codex", [20])));
    expect(useUsage.getState().byAccount.acct_codex?.windows[0]?.usedPct).toBe(20);

    vi.advanceTimersByTime(5 * 60_000);
    await vi.waitFor(() => expect(named("usage_refresh")).toHaveLength(1));
    expect(named("usage_refresh")[0]?.args).toEqual({ accountId: null });

    const status = (s: StatusRecord["status"], ts: number): StatusRecord => ({
      key: "pane:pn_1",
      status: s,
      reason: null,
      since: ts,
      ts,
      tool: null,
      title: null,
      preview: null,
    });
    act(() => useStatus.getState().apply([status("working", 1)], []));
    act(() => useStatus.getState().apply([status("done", 2)], []));
    await vi.waitFor(() => expect(named("usage_refresh")).toHaveLength(2));
    expect(named("usage_refresh")[1]?.args).toEqual({ accountId: "acct_codex" });
    // Another turn within 30 s doesn't probe again.
    act(() => useStatus.getState().apply([status("working", 3)], []));
    act(() => useStatus.getState().apply([status("done", 4)], []));
    expect(named("usage_refresh")).toHaveLength(2);
    stop();
  });
});
