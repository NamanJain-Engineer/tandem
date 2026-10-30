import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  Account,
  AccountOverride,
  EngineView,
  IpcError,
  LoginSession,
  PaneRecord,
  WorkspaceSummary,
} from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { accountForNext, useAccounts } from "../../stores/accounts";
import { useEngines } from "../../stores/engines";
import { useWorkspaces } from "../../stores/workspaces";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { AccountChip } from "../code/AccountChip";
import { AccountsPage } from "./AccountsPage";

vi.mock("../code/MiniTerminal", () => ({
  MiniTerminal: ({ paneId }: { paneId: string }) => <div data-testid="mini-terminal" data-pane-id={paneId} />,
}));

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
  homePath: `/p/${engineId}/${id}`,
  isDefault,
  planLabel: null,
  createdAt: 0,
});

const WORK = account("acc_work", "claude", "Work", true);
const PERSONAL = account("acc_personal", "claude", "Personal");
const STUDIO = account("acc_studio", "codex", "Studio", true);

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

function setup(
  handlers: Parameters<typeof fakeBackend>[0] = {},
  accounts = [WORK, PERSONAL, STUDIO],
  overrides: AccountOverride[] = [],
) {
  calls = fakeBackend({
    accounts_list: () => accounts,
    accounts_overrides: () => overrides,
    accounts_status: (a) =>
      (a as { accountId: string }).accountId === "acc_personal" ? "signed_out" : "signed_in",
    ...handlers,
  });
  useEngines.setState({ list: [engine("claude", "Claude Code"), engine("codex", "Codex")], loaded: true });
  useAccounts.setState({ list: [], overrides: [], loaded: false, failed: false, signIn: {} });
}

beforeEach(() => resetStores());
afterEach(() => setInvokeBackend(null));

describe("accounts (F-AC-02, F-AC-03, UI-35)", () => {
  it("F-AC-03.1 new sessions take the workspace's override, else the engine's default", () => {
    const list = [WORK, PERSONAL, STUDIO];
    expect(accountForNext(list, [], "claude", "ws_shop")?.label).toBe("Work");
    const override: AccountOverride = {
      workspaceId: "ws_shop",
      engineId: "claude",
      accountId: "acc_personal",
    };
    expect(accountForNext(list, [override], "claude", "ws_shop")?.label).toBe("Personal");
    expect(accountForNext(list, [override], "claude", "ws_other")?.label).toBe("Work");
    expect(accountForNext(list, [override], "gemini", "ws_shop")).toBeNull();
  });

  it("UI-35 accounts are grouped by engine with Default, sign-in state, and Make default", async () => {
    setup({ accounts_set_default: () => null });
    render(
      <UiProvider>
        <AccountsPage />
      </UiProvider>,
    );
    const rows = await screen.findAllByTestId("account-row");
    expect(rows.map((r) => r.dataset.label)).toEqual(["Work", "Personal", "Studio"]);
    expect(screen.getByRole("region", { name: "Claude Code" })).toBeTruthy();
    const personal = rows[1] as HTMLElement;
    await waitFor(() => expect(within(personal).getByText("Signed out")).toBeTruthy());
    expect(within(rows[0] as HTMLElement).getByText("Default")).toBeTruthy();
    fireEvent.click(within(personal).getByTestId("account-make-default"));
    await waitFor(() =>
      expect(named("accounts_set_default")[0]?.args).toEqual({ accountId: "acc_personal" }),
    );
  });

  it("F-AC-02.1 Add account signs in in a terminal in the sheet, and Cancel kills it without an account", async () => {
    const session: LoginSession = {
      opId: "login_1",
      paneId: "login_1",
      accountId: "acc_new",
      action: "sign_in",
      reports: true,
    };
    setup({ accounts_begin_add: () => session, accounts_cancel_login: () => null });
    render(
      <UiProvider>
        <AccountsPage />
      </UiProvider>,
    );
    fireEvent.click(await screen.findByTestId("account-add"));
    fireEvent.change(await screen.findByTestId("login-nickname"), { target: { value: "Client A" } });
    fireEvent.click(screen.getByTestId("login-start"));
    await waitFor(() =>
      expect(named("accounts_begin_add")[0]?.args).toEqual({ engineId: "claude", label: "Client A" }),
    );
    expect((await screen.findByTestId("mini-terminal")).dataset.paneId).toBe("login_1");
    expect(
      screen.getByText("You're signing in with Claude Code's own login. Vibezeo never sees your token."),
    ).toBeTruthy();
    fireEvent.click(screen.getByTestId("login-cancel"));
    await waitFor(() => expect(named("accounts_cancel_login")[0]?.args).toEqual({ opId: "login_1" }));
    await waitFor(() => expect(screen.queryByTestId("login-sheet")).toBeNull());
  });

  it("F-AC-02 a finished sign-in says so and Done closes the sheet", async () => {
    const session: LoginSession = {
      opId: "login_2",
      paneId: "login_2",
      accountId: "acc_new",
      action: "sign_in",
      reports: true,
    };
    setup({ accounts_begin_add: () => session });
    render(
      <UiProvider>
        <AccountsPage />
      </UiProvider>,
    );
    fireEvent.click(await screen.findByTestId("account-add"));
    fireEvent.change(await screen.findByTestId("login-nickname"), { target: { value: "Client A" } });
    fireEvent.click(screen.getByTestId("login-start"));
    await screen.findByTestId("mini-terminal");
    act(() =>
      emitLocal("accounts:login-finished", {
        opId: "login_2",
        accountId: "acc_new",
        state: "signed_in",
        message: null,
      }),
    );
    expect(await screen.findByText("Signed in as Client A")).toBeTruthy();
    fireEvent.click(screen.getByTestId("login-done"));
    await waitFor(() => expect(screen.queryByTestId("login-sheet")).toBeNull());
  });

  it("F-AC-02 an accounts-per-engine limit shows the core's reason and sells nothing (D-869)", async () => {
    const limit: IpcError = {
      code: "limit_reached",
      message: "Adding more accounts per engine is turned off for this account.",
      stage: null,
      hint: null,
      retryable: false,
    };
    setup({
      accounts_begin_add: () => {
        throw limit;
      },
    });
    render(
      <UiProvider>
        <AccountsPage />
      </UiProvider>,
    );
    fireEvent.click(await screen.findByTestId("account-add"));
    fireEvent.change(await screen.findByTestId("login-nickname"), { target: { value: "Second" } });
    fireEvent.click(screen.getByTestId("login-start"));
    expect(
      await screen.findByText("Adding more accounts per engine is turned off for this account."),
    ).toBeTruthy();
    expect(screen.queryByTestId("login-upgrade")).toBeNull();
  });

  it("F-AC-03 the AccountChip shows the pane's account and sets new panes' account for the repository", async () => {
    setup({ workspace_set_account_override: () => null });
    const repo = { id: "ws_shop", name: "shop", parentId: null } as unknown as WorkspaceSummary;
    const lane = { id: "ws_lane", name: "fix-auth", parentId: "ws_shop" } as unknown as WorkspaceSummary;
    useWorkspaces.setState({ list: [repo, lane] });
    const pane = {
      id: "pn_1",
      tabId: "tb",
      workspaceId: "ws_lane",
      kind: "agent",
      engineId: "claude",
      accountId: "acc_work",
    } as PaneRecord;
    render(
      <UiProvider>
        <AccountChip pane={pane} />
      </UiProvider>,
    );
    const chip = await screen.findByTestId("account-chip");
    expect(chip.textContent).toBe("Work");
    fireEvent.keyDown(chip, { key: "Enter" });
    fireEvent.click(
      await screen.findByRole("menuitemcheckbox", { name: "Use Personal for new Claude Code panes here" }),
    );
    await waitFor(() =>
      expect(named("workspace_set_account_override")[0]?.args).toEqual({
        id: "ws_shop",
        engineId: "claude",
        accountId: "acc_personal",
      }),
    );
  });

  it("F-AC-03 D-2620 with one account for its engine a pane shows no account chip; a second account brings it", async () => {
    // Every engine's first account is "Default": alone, it tells nothing apart.
    const only = { ...WORK, label: "Default" };
    setup({}, [only, STUDIO]);
    useAccounts.setState({ list: [only, STUDIO], overrides: [], loaded: true, failed: false, signIn: {} });
    const pane = {
      id: "pn_1",
      tabId: "tb",
      workspaceId: "ws_shop",
      kind: "agent",
      engineId: "claude",
      accountId: "acc_work",
    } as PaneRecord;
    render(
      <UiProvider>
        <AccountChip pane={pane} />
      </UiProvider>,
    );
    expect(screen.queryByTestId("account-chip")).toBeNull();
    act(() => useAccounts.setState({ list: [only, PERSONAL, STUDIO] }));
    expect((await screen.findByTestId("account-chip")).textContent).toBe("Default");
  });

  it("F-AC-03 the chip reloads once for an account created after the list loaded", async () => {
    setup();
    useAccounts.setState({ list: [STUDIO], overrides: [], loaded: true, failed: false, signIn: {} });
    const pane = {
      id: "pn_9",
      tabId: "tb",
      workspaceId: "ws_shop",
      kind: "agent",
      engineId: "claude",
      accountId: "acc_work",
    } as PaneRecord;
    render(
      <UiProvider>
        <AccountChip pane={pane} />
      </UiProvider>,
    );
    expect((await screen.findByTestId("account-chip")).textContent).toBe("Work");
    expect(named("accounts_list")).toHaveLength(1);
  });
});
