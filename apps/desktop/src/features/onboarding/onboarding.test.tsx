import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useLayout } from "../../stores/layout";
import { useOnboarding } from "../../stores/onboarding";
import { useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { Onboarding } from "./Onboarding";

const STEPS = ["engines", "privacy", "workspace"];

const WS = (root: string): WorkspaceSummary => ({
  id: "ws_1",
  kind: "local",
  hostId: "local",
  root,
  wslDistro: null,
  name: "shop",
  parentId: null,
  laneBranch: null,
  laneBase: null,
  sortKey: "a0",
  pinned: false,
  collapsed: false,
  sessionsCollapsed: false,
  createdAt: 0,
  missing: false,
  branch: null,
  liveCount: 0,
  tabs: [{ id: "tb_1", workspaceId: "ws_1", name: "Tab 1", sortKey: "a0", createdAt: 0 }],
});

function setup(): Call[] {
  let step: string | null = "engines";
  let list: WorkspaceSummary[] = [];
  return fakeBackend({
    onboarding_set: (a) => {
      step = a?.done ? null : String(a?.step);
      return { step, done: step === null, steps: STEPS };
    },
    engines_list: () => [
      { spec: { id: "claude", label: "Claude Code", logo: "C" }, install: { path: "/bin/claude", version: "1.2.3", source: "shell" }, chatCapable: false, unsupportedReason: null, extraArgs: [] },
      { spec: { id: "kimi", label: "Kimi", logo: "K" }, install: null, chatCapable: false, unsupportedReason: null, extraArgs: [] },
    ],
    engines_report: () => null,
    workspace_pick_folder: () => "/code/shop",
    workspace_add: (a) => {
      list = [WS(String(a?.root))];
      return { workspace: list[0], created: true };
    },
    workspace_list: () => list,
  });
}

function mount() {
  return render(
    <UiProvider>
      <Onboarding />
    </UiProvider>,
  );
}

describe("onboarding wizard (F-SH-08)", () => {
  beforeEach(() => {
    resetStores();
    useOnboarding.setState({ state: { step: "engines", done: false, steps: STEPS } });
  });

  it("F-SH-08 the engines step lists what's installed; Skip and Back move through persisted steps", async () => {
    const calls = setup();
    mount();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Set up your coding agents");
    await waitFor(() => expect(screen.getByTestId("onboarding-engines").textContent).toContain("Installed 1.2.3"));
    expect((screen.getByTestId("onboarding-back") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByTestId("onboarding-skip"));
    await waitFor(() => expect(screen.getByTestId("onboarding").dataset.step).toBe("privacy"));
    fireEvent.click(screen.getByTestId("onboarding-back"));
    await waitFor(() => expect(screen.getByTestId("onboarding").dataset.step).toBe("engines"));
    expect(calls.filter((c) => c.cmd === "onboarding_set").map((c) => c.args?.step)).toEqual(["privacy", "engines"]);
  });

  it("F-TL-01 the privacy step asks once with both toggles off; turning one on saves it", async () => {
    const calls = setup();
    useOnboarding.setState({ state: { step: "privacy", done: false, steps: STEPS } });
    mount();
    const usage = screen.getByRole("switch", { name: "Share anonymous usage statistics" });
    const crash = screen.getByRole("switch", { name: "Send crash reports" });
    expect(usage.getAttribute("aria-checked")).toBe("false");
    expect(crash.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(crash);
    await waitFor(() => expect(calls.find((c) => c.cmd === "settings_set")?.args).toEqual({ key: "telemetry_crash", value: true }));
    await waitFor(() => expect(useSettings.getState().values.telemetry_crash).toBe(true));
  });

  it("F-SH-08.1 choosing a folder finishes in Code mode on that workspace's launch grid", async () => {
    const calls = setup();
    useOnboarding.setState({ state: { step: "workspace", done: false, steps: STEPS } });
    useUi.setState({ mode: "thread" });
    mount();
    fireEvent.click(screen.getByTestId("onboarding-choose-folder"));
    await waitFor(() => expect(useOnboarding.getState().state?.done).toBe(true));
    expect(calls.filter((c) => c.cmd === "onboarding_set").at(-1)?.args).toEqual({ done: true });
    expect(useLayout.getState().tabs.tb_1?.keepEmpty).toBe(true);
    expect(useUi.getState().mode).toBe("code");
  });

  it("F-SH-08.3 skipping the last step finishes without a workspace", async () => {
    setup();
    useOnboarding.setState({ state: { step: "workspace", done: false, steps: STEPS } });
    mount();
    expect(screen.queryByTestId("onboarding-continue")).toBeNull();
    fireEvent.click(screen.getByTestId("onboarding-skip"));
    await waitFor(() => expect(useOnboarding.getState().state?.done).toBe(true));
  });
});
