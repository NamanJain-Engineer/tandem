import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { axe } from "vitest-axe";
import type { CallLogEntry, CallView, ChipView } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { IDLE, performUiAction, useConductor } from "../../stores/conductor";
import { useSession } from "../../stores/session";
import { useSettings } from "../../stores/settings";
import { useSidebar } from "../../stores/sidebar";
import { useUi } from "../../stores/ui";
import { FREE, FREE_VIEW, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { runCommand } from "../commands/commands";
import { CallLog } from "./CallLog";
import { ConductorCallSettings } from "./CallSettingsSlot";
import { FloatingRing } from "./FloatingRing";
import { ConductorButton } from "./ConductorButton";
import { Hud } from "./Hud";
import { foldChips } from "./ring";

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

const chip = (i: number, over: Partial<ChipView> = {}): ChipView => ({
  id: `c${i}`,
  tool: "switch_mode",
  group: "navigation",
  label: `Action ${i}`,
  state: "done",
  reason: null,
  ...over,
});

const live = (over: Partial<CallView> = {}): CallView => ({
  ...IDLE,
  callId: "vs_1",
  phase: "listening",
  maxSeconds: 1800,
  elapsedMs: 252_000,
  ...over,
});

function pro(): void {
  useSession.setState({
    entitlement: {
      ...FREE_VIEW,
      plan: "pro",
      limits: { ...FREE, conductor: true },
      credits: { monthly: 3_120, purchased: 0, resetsAt: null, cap: null, spent: null, grant: null },
    },
  });
}

function hud(view: CallView) {
  act(() => useConductor.getState().setView(view));
  return render(
    <UiProvider>
      <Hud />
    </UiProvider>,
  );
}

beforeEach(() => {
  resetStores();
  useConductor.setState({ view: IDLE, level: 0, logOpen: false });
  calls = fakeBackend({ conductor_start_call: () => live({ phase: "connecting" }) });
});

describe("Conductor HUD (F-CN-04, UI-61)", () => {
  it("F-CN-04.1 after 5 actions, 3 chips and +2 show", () => {
    hud(live({ chips: [1, 2, 3, 4, 5].map((i) => chip(i)) }));
    const shown = screen.getAllByTestId("conductor-chip").map((c) => c.textContent);
    expect(shown).toEqual(["Action 3", "Action 4", "Action 5"]);
    expect(screen.getByTestId("conductor-more").textContent).toBe("+2");
    expect(foldChips([1, 2]).folded).toEqual([]);
  });

  it("F-CN-04 shows the state, the last utterance and the time left", async () => {
    pro();
    const { container } = hud(live({ phase: "thinking", utterance: "open the auth pane" }));
    expect(screen.getByTestId("conductor-phase").textContent).toBe("Thinking");
    expect(screen.getByTestId("conductor-utterance").textContent).toBe("“open the auth pane”");
    // 3,120 credits at about 120 a minute: 26 minutes.
    expect(screen.getByTestId("conductor-meter").textContent).toBe("◈ 4:12 · ~26 min");
    expect(await axe(container)).toHaveNoViolations();
  });

  it("F-CN-02.3 a shell command's question says it needs a click, and the click names its request", async () => {
    hud(
      live({
        question: {
          requestId: "q-1-1",
          tool: "run_command_in_shell",
          summary: "Run `pnpm test` in zsh?",
          verb: "run it",
          review: "screen_only",
        },
      }),
    );
    expect(screen.getByTestId("conductor-question").textContent).toContain("Confirm on screen");
    fireEvent.click(screen.getByTestId("conductor-yes"));
    await waitFor(() => expect(named("conductor_confirm")).toHaveLength(1));
    expect(named("conductor_confirm")[0]?.args).toEqual({ requestId: "q-1-1", yes: true });
    expect(screen.getByTestId("conductor-yes").textContent).toBe("Yes, run it");
  });

  it("F-CN-01 startup errors show with what to do; dismissing clears them", async () => {
    hud(
      live({
        phase: "error",
        error: { code: "time_cap", message: "This call reached its 30-minute limit." },
      }),
    );
    expect(screen.getByTestId("conductor-error-message").textContent).toBe(
      "This call reached its 30-minute limit.",
    );
    expect(screen.getByTestId("conductor-error-action").textContent).toBe("Start a new call");
    fireEvent.click(screen.getByTestId("conductor-dismiss"));
    await waitFor(() => expect(useConductor.getState().view.phase).toBe("idle"));
    expect(named("conductor_end_call")).toHaveLength(1);
  });

  it("F-CN-01.1 the receipt shows time, credits and actions", () => {
    hud(live({ phase: "ended", receipt: { callId: "vs_1", seconds: 65, credits: 140, actions: 5 } }));
    expect(screen.getByTestId("conductor-receipt").textContent).toContain(
      "Call ended · 1:05 · 140 credits · 5 actions",
    );
  });

  it("F-CN-04 the HUD is hidden with no call", () => {
    hud(IDLE);
    expect(screen.queryByTestId("conductor-hud")).toBeNull();
  });
});

describe("Conductor ring and commands (F-CN-01, F-CN-04)", () => {
  it("F-CN-04 the ring is hidden on Free, where the call command opens the plans", () => {
    render(
      <UiProvider>
        <ConductorButton />
      </UiProvider>,
    );
    expect(screen.queryByTestId("conductor-button")).toBeNull();
    runCommand("conductor.call");
    expect(useUi.getState().upgrade.open).toBe(true);
    expect(named("conductor_start_call")).toHaveLength(0);
  });

  it("F-CN-01 on Pro a click starts a call", async () => {
    pro();
    render(
      <UiProvider>
        <ConductorButton />
      </UiProvider>,
    );
    const b = screen.getByTestId("conductor-button");
    fireEvent.pointerDown(b);
    fireEvent.pointerUp(b);
    await waitFor(() => expect(named("conductor_start_call")).toHaveLength(1));
    expect(named("conductor_start_call")[0]?.args).toEqual({ wake: false, pushToTalk: false });
    expect(useConductor.getState().view.phase).toBe("connecting");
  });

  it("F-CN-04.2 the ring stops drawing while the window is hidden", () => {
    pro();
    act(() => useConductor.getState().setView(live()));
    const hidden = Object.getOwnPropertyDescriptor(Document.prototype, "visibilityState");
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
    try {
      render(
        <UiProvider>
          <ConductorButton />
        </UiProvider>,
      );
      act(() => {
        document.dispatchEvent(new Event("visibilitychange"));
      });
      expect(screen.getByTestId("waveform-ring").hasAttribute("data-paused")).toBe(true);
    } finally {
      delete (document as unknown as Record<string, unknown>).visibilityState;
      if (hidden) Object.defineProperty(Document.prototype, "visibilityState", hidden);
    }
  });

  it("F-CN-02 navigation tools act in the window", () => {
    performUiAction({ action: "switch_mode", args: { mode: "thread" } });
    expect(useUi.getState().mode).toBe("thread");
    performUiAction({ action: "open_sidebar_tab", args: { tab: "files" } });
    expect(useSidebar.getState().open).toBe(true);
    performUiAction({ action: "unknown_tool", args: {} });
  });
});

describe("Call log (F-CN-04, UI-66)", () => {
  const entry: CallLogEntry = {
    call: {
      id: "vs_1",
      startedAt: 1_760_000_000_000,
      endedAt: 1_760_000_065_000,
      connectedSeconds: 65,
      credits: 140,
      endReason: "hung_up",
      actions: 1,
    },
    events: [
      { ts: 1_760_000_001_000, kind: "user", text: "open code mode", ok: null },
      { ts: 1_760_000_002_000, kind: "action", text: "Code mode: Done.", ok: true },
      { ts: 1_760_000_003_000, kind: "assistant", text: "Done.", ok: null },
    ],
  };

  function log(entries: CallLogEntry[] | Error) {
    calls = fakeBackend({
      conductor_call_log: () => {
        if (entries instanceof Error) throw { code: "io", message: entries.message };
        return entries;
      },
    });
    render(
      <UiProvider>
        <CallLog />
      </UiProvider>,
    );
    act(() => useConductor.getState().openLog(true));
  }

  it("F-CN-04 empty: no calls yet", async () => {
    log([]);
    expect(await screen.findByText("No calls yet")).toBeTruthy();
  });

  it("F-CN-04 a call's turns and actions, in order", async () => {
    log([entry]);
    await screen.findByTestId("call-log-events");
    const items = screen.getByTestId("call-log-events").querySelectorAll("li");
    expect(Array.from(items).map((li) => li.getAttribute("data-kind"))).toEqual([
      "user",
      "action",
      "assistant",
    ]);
    expect(items[0]?.textContent).toContain("You: open code mode");
    expect(screen.getByText(/1:05 · 140 credits/)).toBeTruthy();
    fireEvent.click(screen.getByTestId("call-log-delete"));
    await waitFor(() => expect(named("conductor_call_delete")[0]?.args).toEqual({ callId: "vs_1" }));
  });

  it("F-CN-04 a failed load says so and offers a retry", async () => {
    log(new Error("The database is busy."));
    expect(await screen.findByText("The database is busy.")).toBeTruthy();
  });
});

describe("Conductor settings (F-CN-04, UI-67)", () => {
  it("F-CN-04 keep call logs: absent on Free; on Pro it stores days or off", async () => {
    const { unmount } = render(
      <UiProvider>
        <ConductorCallSettings />
      </UiProvider>,
    );
    expect(screen.queryByText("Keep call logs")).toBeNull();
    unmount();
    pro();
    render(
      <UiProvider>
        <ConductorCallSettings />
      </UiProvider>,
    );
    expect(screen.getByText("Keep call logs")).toBeTruthy();
    expect(screen.getByRole("combobox", { name: "Keep call logs" }).textContent).toContain("7 days");
    await act(async () => {
      await useSettings.getState().set("conductor_call_logs", "off");
    });
    expect(named("settings_set").at(-1)?.args).toEqual({ key: "conductor_call_logs", value: "off" });
    expect(screen.getByRole("combobox", { name: "Keep call logs" }).textContent).toContain("Don't keep");
  });
});

describe("Floating ring (F-CN-04, UI-61)", () => {
  it("F-CN-04 a click toggles the call; hovering shows Mute, Call log and End", async () => {
    const { rerender } = render(
      <UiProvider>
        <FloatingRing view={IDLE} level={0} />
      </UiProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Talk to Conductor" }));
    await waitFor(() => expect(named("conductor_start_call")[0]?.args).toEqual({ wake: false }));
    rerender(
      <UiProvider>
        <FloatingRing view={live()} level={0.4} />
      </UiProvider>,
    );
    expect(screen.queryByRole("button", { name: "Mute" })).toBeNull();
    fireEvent.mouseEnter(screen.getByTestId("floating-ring"));
    fireEvent.click(screen.getByRole("button", { name: "Mute" }));
    await waitFor(() => expect(named("conductor_mute")[0]?.args).toEqual({ muted: true }));
    expect(screen.getByRole("button", { name: "Call log" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "End" }));
    await waitFor(() => expect(named("conductor_end_call")).toHaveLength(1));
  });

  it("F-CN-04 the floating ring switch is a Pro setting", async () => {
    pro();
    render(
      <UiProvider>
        <ConductorCallSettings />
      </UiProvider>,
    );
    fireEvent.click(screen.getByRole("switch", { name: "Floating ring" }));
    await waitFor(() =>
      expect(named("settings_set").at(-1)?.args).toEqual({ key: "conductor_floating_ring", value: true }),
    );
  });
});
