import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { WorkspaceSummary } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useLayout } from "../../stores/layout";
import { useOnboarding } from "../../stores/onboarding";
import { useSettings } from "../../stores/settings";
import { useUi } from "../../stores/ui";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { Onboarding } from "./Onboarding";

const STEPS = ["engines", "accounts", "privacy", "workspace"];

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

function setup(handlers: Parameters<typeof fakeBackend>[0] = {}): Call[] {
  let step: string | null = "engines";
  let list: WorkspaceSummary[] = [];
  return fakeBackend({
    onboarding_set: (a) => {
      step = a?.done ? null : String(a?.step);
      return { step, done: step === null, steps: STEPS };
    },
    engines_list: () => [
      {
        spec: { id: "claude", label: "Claude Code", logo: "C", chatAdapter: "claude", homeStrategy: "env" },
        install: { path: "/bin/claude", version: "1.2.3", source: "shell" },
        chatCapable: false,
        unsupportedReason: null,
        extraArgs: [],
      },
      {
        spec: { id: "codex", label: "Codex", logo: "X", chatAdapter: "codex", homeStrategy: "env" },
        install: { path: "/bin/codex", version: "0.52.0", source: "shell" },
        chatCapable: false,
        unsupportedReason: null,
        extraArgs: [],
      },
      // Signs in globally: its accounts can't be kept apart.
      {
        spec: {
          id: "cursor-agent",
          label: "Cursor Agent",
          logo: "Cu",
          chatAdapter: "acp",
          homeStrategy: "none",
        },
        install: { path: "/bin/cursor-agent", version: "1.0.0", source: "shell" },
        chatCapable: false,
        unsupportedReason: null,
        extraArgs: [],
      },
      {
        spec: { id: "kimi", label: "Kimi", logo: "K", chatAdapter: null, homeStrategy: "none" },
        install: null,
        chatCapable: false,
        unsupportedReason: null,
        extraArgs: [],
      },
    ],
    engines_report: () => null,
    workspace_pick_folder: () => "/code/shop",
    workspace_add: (a) => {
      list = [WS(String(a?.root))];
      return { workspace: list[0], created: true };
    },
    workspace_list: () => list,
    accounts_list: () => [],
    accounts_overrides: () => [],
    ...handlers,
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
    await waitFor(() =>
      expect(screen.getByTestId("onboarding-engines").textContent).toContain("Installed 1.2.3"),
    );
    expect((screen.getByTestId("onboarding-back") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByTestId("onboarding-skip"));
    await waitFor(() => expect(screen.getByTestId("onboarding").dataset.step).toBe("accounts"));
    fireEvent.click(screen.getByTestId("onboarding-back"));
    await waitFor(() => expect(screen.getByTestId("onboarding").dataset.step).toBe("engines"));
    expect(calls.filter((c) => c.cmd === "onboarding_set").map((c) => c.args?.step)).toEqual([
      "accounts",
      "engines",
    ]);
  });

  it("F-SH-08 F-AC-02 the accounts step offers each installed chat engine's own sign-in, and says when one is signed in", async () => {
    const work = {
      id: "acct_c",
      engineId: "claude",
      label: "Work",
      homePath: "/p/acct_c",
      isDefault: true,
      planLabel: null,
      createdAt: 0,
    };
    const calls = setup({ accounts_list: () => [work], accounts_status: () => "signed_in" });
    useOnboarding.setState({ state: { step: "accounts", done: false, steps: STEPS } });
    mount();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Sign in to your agents");
    const rows = await screen.findByTestId("onboarding-accounts");
    // Installed chat engines whose accounts stay apart: not Cursor Agent, not Kimi.
    expect(Array.from(rows.querySelectorAll("li")).map((li) => li.dataset.engine)).toEqual([
      "claude",
      "codex",
    ]);
    const claude = rows.querySelector<HTMLElement>('[data-engine="claude"]') as HTMLElement;
    await waitFor(() => expect(claude.dataset.signedIn).toBe("true"));
    expect(claude.textContent).toContain("Signed in");
    expect(within(claude).queryByRole("button")).toBeNull();

    // Codex has no account yet: its sign-in starts as a new account, named first.
    fireEvent.click(screen.getByRole("button", { name: "Sign in to Codex" }));
    const nickname = (await screen.findByTestId("login-nickname")) as HTMLInputElement;
    expect(nickname.value).toBe("Personal");
    expect(calls.some((c) => c.cmd === "accounts_begin_add")).toBe(false);
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
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "settings_set")?.args).toEqual({
        key: "telemetry_crash",
        value: true,
      }),
    );
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
